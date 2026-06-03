/*
 * Comando: fix-db-auth
 * Detecta y corrige un mismatch entre la contraseña en DATABASE_URL y el hash
 * almacenado en PostgreSQL. Ocurre cuando Coolify regenera SERVICE_PASSWORD_POSTGRES
 * al hacer un redeploy/rebuild.
 *
 * Flujo:
 * 1. Leer credenciales del .env en disco del servidor (auto-detecta esquema nuevo vs legacy)
 *    - Esquema nuevo: SERVICE_PASSWORD_POSTGRES → rust_app/rust_db
 *    - Esquema legacy: DB_PASSWORD + parsear DATABASE_URL del compose → user/db detectados
 * 2. Aplicar ALTER USER via unix socket (trust auth) para actualizar el hash
 * 3. Corregir DATABASE_URL en docker-compose.yml para usar container_name en lugar
 *    del service name genérico "postgres" — evita colisiones DNS cuando el container
 *    está en múltiples redes (coolify + red del servicio).
 * 4. Recrear el contenedor app con la configuración corregida
 * 5. Verificar health
 *
 * Gotcha principal: el hostname "postgres" puede resolver a coolify-db (el postgres
 * propio de Coolify) cuando el container app está en la red "coolify" (compartida).
 * El container_name (ej: postgres-{uuid}) es globalmente único y siempre resuelve al
 * postgres correcto.
 *
 * Este bug afectó al sitio glory-rest (restaurante.wandori.us) el 2026-05-11 cuando
 * Coolify regeneró DB_PASSWORD en un redeploy. La app usaba @postgres: (colisión DNS)
 * en lugar de @postgres-{uuid}: y se conectaba al postgres de Coolify con credenciales
 * incorrectas. fix-db-auth corrige ambos problemas (hash + hostname) en un solo paso.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::health_manager;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    dry_run: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    let stack_uuid = site.stack_uuid.as_deref().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{site_name}' sin stackUuid configurado"))
    })?;
    let target = settings.resolve_site_target(site)?;
    let service_dir = format!("/data/coolify/services/{stack_uuid}");

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    /* --- 1. Leer contraseña actual del .env en el servidor --- */
    println!("[1/5] Leyendo SERVICE_PASSWORD_POSTGRES del servidor...");
    let env_content = ssh
        .execute(&format!("cat {service_dir}/.env 2>/dev/null || echo ''"))
        .await?;

    /* Auto-detectar esquema de credenciales:
     * - Nuevo (rust-stack): SERVICE_PASSWORD_POSTGRES → rust_app / rust_db
     * - Legacy (glory-rest y similares): DB_PASSWORD + parsear DATABASE_URL del compose */
    let (password, db_user, db_name) =
        detect_db_credentials(&ssh, &env_content.stdout, &service_dir).await?;

    let postgres_container = format!("postgres-{stack_uuid}");
    let app_container = format!("app-{stack_uuid}");

    println!(
        "      Contraseña encontrada ({}...) — usuario={db_user} db={db_name}",
        &password[..8.min(password.len())]
    );
    println!("      Postgres container: {postgres_container}");

    /* --- 2. Verificar si el mismatch realmente existe --- */
    println!("[2/5] Verificando autenticación actual...");
    let auth_check =
        test_postgres_auth(&ssh, &postgres_container, &password, &db_user, &db_name).await?;

    if auth_check {
        println!("      Auth OK — no hay mismatch detectado.");
        if !dry_run {
            /* Aun así, corregir el DATABASE_URL por si tiene el hostname genérico */
            let fixed = fix_database_url_hostname(&ssh, &service_dir, stack_uuid).await?;
            if fixed {
                println!("      DATABASE_URL corregido para usar container_name.");
                restart_app_container(&ssh, &service_dir).await?;
            }
        }
        return Ok(());
    }

    println!("      Mismatch detectado — aplicando corrección...");

    if dry_run {
        println!("[dry-run] Se aplicaría: ALTER USER + fix DATABASE_URL + restart app");
        return Ok(());
    }

    /* --- 3. ALTER USER via unix socket (trust auth) --- */
    println!("[3/5] Actualizando hash de contraseña en PostgreSQL...");
    let sql = format!(
        "ALTER USER {} WITH PASSWORD '{}';",
        db_user,
        escape_sql_string(&password)
    );
    let encoded = base64_encode(sql.as_bytes());
    let alter_cmd = format!(
        "echo {} | base64 -d | docker exec -i {} psql -U {} -d {}",
        encoded, postgres_container, db_user, db_name
    );
    let result = ssh.execute(&alter_cmd).await?;
    if !result.stdout.contains("ALTER ROLE") && result.exit_code != 0 {
        return Err(CoolifyError::Validation(format!(
            "ALTER USER falló: stdout={} stderr={}",
            result.stdout.trim(),
            result.stderr.trim()
        )));
    }
    println!("      ALTER ROLE ejecutado correctamente.");

    /* --- 4. Verificar que la auth funciona ahora --- */
    let verified =
        test_postgres_auth(&ssh, &postgres_container, &password, &db_user, &db_name).await?;
    if !verified {
        return Err(CoolifyError::Validation(
            "ALTER USER ejecutado pero la auth sigue fallando — revisar pg_hba.conf".into(),
        ));
    }
    println!("      Auth verificada con éxito.");

    /* --- 4b. Corregir DATABASE_URL para usar container_name único --- */
    println!("[4/5] Corrigiendo DATABASE_URL en docker-compose.yml...");
    let fixed = fix_database_url_hostname(&ssh, &service_dir, stack_uuid).await?;
    if fixed {
        println!("      DATABASE_URL actualizado: @postgres → @{postgres_container}");
    } else {
        println!("      DATABASE_URL ya usa container_name — sin cambios.");
    }

    /* --- 5. Recrear contenedor app con la configuración corregida --- */
    println!("[5/5] Reiniciando contenedor app...");
    restart_app_container(&ssh, &service_dir).await?;

    /* Esperar a que el contenedor levante */
    tokio::time::sleep(std::time::Duration::from_secs(8)).await;

    /* Confirmar que el app conectó bien */
    let app_logs = ssh
        .execute(&format!("docker logs {} --tail 5 2>&1", app_container))
        .await?;

    if app_logs.stdout.contains("password authentication failed")
        || app_logs.stderr.contains("password authentication failed")
    {
        return Err(CoolifyError::Validation(
            "App sigue fallando auth después del fix — ver logs con: logs --name {name} --target app".into(),
        ));
    }

    if app_logs.stdout.contains("Servidor iniciando") || app_logs.stdout.contains("listening") {
        println!("      App arrancó correctamente.");
    }

    /* Health check final */
    let report = health_manager::assert_site_healthy(&settings, site, &ssh).await?;
    if report.healthy() {
        println!("\nfix-db-auth completado — '{site_name}' está healthy.");
    } else {
        for detail in &report.details {
            println!("  - {detail}");
        }
        return Err(CoolifyError::Validation(format!(
            "Fix aplicado pero health check falló para '{site_name}'"
        )));
    }

    Ok(())
}

/* ---------- helpers ------------------------------------------------- */

/* [145A-1] Auto-detecta el esquema de credenciales del stack:
 * - Nuevo (rust-stack): SERVICE_PASSWORD_POSTGRES → user=rust_app, db=rust_db
 * - Legacy (glory-rest y variantes): DB_PASSWORD → parsear DATABASE_URL del compose
 *   para obtener user y db reales (pueden ser glory_app/glory u otros).
 * Retorna (password, db_user, db_name). */
async fn detect_db_credentials(
    ssh: &SshClient,
    env_content: &str,
    service_dir: &str,
) -> std::result::Result<(String, String, String), CoolifyError> {
    /* Intento 1: esquema nuevo */
    if let Some(password) = parse_env_value(env_content, "SERVICE_PASSWORD_POSTGRES") {
        return Ok((password, "rust_app".to_string(), "rust_db".to_string()));
    }

    /* Intento 2: esquema legacy — DB_PASSWORD + DATABASE_URL en compose */
    let password = parse_env_value(env_content, "DB_PASSWORD").ok_or_else(|| {
        CoolifyError::Validation(
            "No se encontró SERVICE_PASSWORD_POSTGRES ni DB_PASSWORD en el .env del servidor — \
             asegúrate de que el sitio tiene stackUuid y .env en /data/coolify/services/{uuid}/"
                .into(),
        )
    })?;

    /* Parsear DATABASE_URL del compose para extraer usuario y base de datos */
    let compose_content = ssh
        .execute(&format!(
            "cat {service_dir}/docker-compose.yml 2>/dev/null || echo ''"
        ))
        .await?;

    let (db_user, db_name) = extract_user_db_from_compose(&compose_content.stdout)
        .unwrap_or_else(|| ("glory_app".to_string(), "glory".to_string()));

    Ok((password, db_user, db_name))
}

/* Extrae el usuario y la base de datos de la primera línea DATABASE_URL encontrada en el compose.
 * Soporta formatos: postgres://user:pass@host:port/db y postgres://user:${VAR}@host:port/db */
pub fn extract_user_db_from_compose(compose: &str) -> Option<(String, String)> {
    for line in compose.lines() {
        let trimmed = line.trim();
        /* Buscar líneas que contengan DATABASE_URL */
        if !trimmed.contains("DATABASE_URL") {
            continue;
        }
        /* Aislar la URL postgres:// */
        let start = trimmed.find("postgres://")?;
        let url = &trimmed[start..];
        /* Quitar comillas al final y espacios */
        let url = url.trim_end_matches('\'').trim_end_matches('"').trim();
        /* postgres://user:password@host:port/db */
        let after_scheme = url.strip_prefix("postgres://")?;
        /* Separar user:pass@rest */
        let at_pos = after_scheme.find('@')?;
        let user_pass = &after_scheme[..at_pos];
        let colon_pos = user_pass.find(':')?;
        let db_user = user_pass[..colon_pos].to_string();
        /* Separar host:port/db */
        let rest = &after_scheme[at_pos + 1..];
        let slash_pos = rest.find('/')?;
        let db_name_raw = &rest[slash_pos + 1..];
        /* Quitar parámetros de conexión si existen (?sslmode=...) */
        let db_name = db_name_raw
            .split('?')
            .next()
            .unwrap_or(db_name_raw)
            .trim_end_matches('\'')
            .trim_end_matches('"')
            .to_string();
        if !db_user.is_empty() && !db_name.is_empty() {
            return Some((db_user, db_name));
        }
    }
    None
}

/// Testea auth TCP (SCRAM) desde un container postgres:alpine temporal en la misma red.
async fn test_postgres_auth(
    ssh: &SshClient,
    postgres_container: &str,
    password: &str,
    db_user: &str,
    db_name: &str,
) -> std::result::Result<bool, CoolifyError> {
    /* Extraer el UUID de red del container name (postgres-{uuid}) */
    let uuid = postgres_container
        .strip_prefix("postgres-")
        .unwrap_or(postgres_container);

    let script = format!(
        "docker run --rm --network {} -e PGPASSWORD='{}' postgres:16-alpine psql -h {} -U {} -d {} -c 'SELECT 1;' 2>&1 | grep -q '1 row' && echo AUTH_OK || echo AUTH_FAIL",
        uuid,
        escape_shell_single_quote(password),
        postgres_container,
        db_user,
        db_name
    );
    let encoded = base64_encode(script.as_bytes());
    let result = ssh
        .execute(&format!("echo {} | base64 -d | bash", encoded))
        .await?;

    Ok(result.stdout.contains("AUTH_OK"))
}

/// Reemplaza el hostname genérico "postgres" en DATABASE_URL por el container_name.
/// Retorna true si hizo algún cambio.
async fn fix_database_url_hostname(
    ssh: &SshClient,
    service_dir: &str,
    stack_uuid: &str,
) -> std::result::Result<bool, CoolifyError> {
    let compose_file = format!("{service_dir}/docker-compose.yml");
    let postgres_container = format!("postgres-{stack_uuid}");

    /* Comprobar si ya usa el container_name */
    let check = ssh
        .execute(&format!(
            "if grep -q '@{}:' {} 2>/dev/null; then echo ALREADY_FIXED; else echo NEEDS_FIX; fi",
            postgres_container, compose_file
        ))
        .await?;

    if database_url_already_uses_container_name(&check.stdout) {
        return Ok(false);
    }

    /* Reemplazar @postgres: por @postgres-{uuid}: en DATABASE_URL */
    let sed_cmd = format!(
        "sed -i 's|@postgres:|@{}:|g' {}",
        postgres_container, compose_file
    );
    let result = ssh.execute(&sed_cmd).await?;
    if result.exit_code != 0 {
        return Err(CoolifyError::Validation(format!(
            "No se pudo actualizar DATABASE_URL en docker-compose.yml: {}",
            result.stderr.trim()
        )));
    }

    Ok(true)
}

fn database_url_already_uses_container_name(output: &str) -> bool {
    output.lines().any(|line| line.trim() == "ALREADY_FIXED")
}

/// Recrea el contenedor app con la nueva configuración del compose.
async fn restart_app_container(
    ssh: &SshClient,
    service_dir: &str,
) -> std::result::Result<(), CoolifyError> {
    let cmd = format!("cd {} && docker compose up -d app 2>&1", service_dir);
    let result = ssh.execute(&cmd).await?;
    if !result.stdout.contains("Started") && result.exit_code != 0 {
        return Err(CoolifyError::Validation(format!(
            "docker compose up -d app falló: {}",
            result.stdout.trim()
        )));
    }
    Ok(())
}

/// Parsea el valor de una variable del contenido de un .env.
fn parse_env_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(&format!("{}=", key)) {
            /* Quitar comillas si las tiene */
            let val = rest.trim_matches('"').trim_matches('\'').to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

/// Escapa comillas simples para interpolación en SQL.
fn escape_sql_string(s: &str) -> String {
    s.replace('\'', "''")
}

/// Escapa comillas simples para un argumento dentro de `'...'` en shell.
fn escape_shell_single_quote(s: &str) -> String {
    s.replace('\'', "'\\''")
}

/// Codifica bytes a base64 estándar.
fn base64_encode(data: &[u8]) -> String {
    use std::fmt::Write;
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 {
            chunk[1] as usize
        } else {
            0
        };
        let b2 = if chunk.len() > 2 {
            chunk[2] as usize
        } else {
            0
        };
        let _ = write!(out, "{}", TABLE[b0 >> 2] as char);
        let _ = write!(out, "{}", TABLE[((b0 & 3) << 4) | (b1 >> 4)] as char);
        if chunk.len() > 1 {
            let _ = write!(out, "{}", TABLE[((b1 & 0xf) << 2) | (b2 >> 6)] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            let _ = write!(out, "{}", TABLE[b2 & 0x3f] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_url_check_detects_already_fixed() {
        assert!(database_url_already_uses_container_name("ALREADY_FIXED\n"));
        assert!(!database_url_already_uses_container_name("NEEDS_FIX\n"));
        assert!(!database_url_already_uses_container_name("0\n0\n"));
    }

    #[test]
    fn parse_env_value_reads_simple_values() {
        let content = "FOO=bar\nSERVICE_PASSWORD_POSTGRES=secret123\n";
        assert_eq!(
            parse_env_value(content, "SERVICE_PASSWORD_POSTGRES"),
            Some("secret123".to_string())
        );
    }
}
