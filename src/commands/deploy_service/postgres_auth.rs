use crate::commands::fix_db_auth::extract_user_db_from_compose;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

pub(crate) async fn ensure_postgres_auth_and_hostname(
    ssh: &SshClient,
    service_dir: &str,
    stack_uuid: &str,
) -> std::result::Result<(), CoolifyError> {
    let env_content = ssh
        .execute(&format!("cat {service_dir}/.env 2>/dev/null || true"))
        .await?;
    /* [095A-23] Soportar esquema legacy: DB_PASSWORD en lugar de SERVICE_PASSWORD_POSTGRES.
     * glory-rest y variantes usan DB_PASSWORD + DATABASE_URL en compose.
     * Nuevo (rust-stack): SERVICE_PASSWORD_POSTGRES -> user=rust_app, db=rust_db.
     * Legacy: DB_PASSWORD -> parsear DATABASE_URL del compose para user/db.
     */
    let (password, db_user, db_name) =
        if let Some(pw) = parse_env_value(&env_content.stdout, "SERVICE_PASSWORD_POSTGRES") {
            /* [0107-1] Parsear DATABASE_URL del compose para extraer usuario y base de datos
             * reales en vez de hardcodear rust_app/rust_db — stacks como kamples usan
             * credenciales distintas (kamples/kamples). Fallback a rust_app/rust_db. */
            let compose_content = ssh
                .execute(&format!(
                    "cat {service_dir}/docker-compose.yml 2>/dev/null || echo ''"
                ))
                .await?;
            let (user, db) = extract_user_db_from_compose(&compose_content.stdout)
                .unwrap_or_else(|| ("rust_app".to_string(), "rust_db".to_string()));
            (pw, user, db)
        } else if let Some(pw) = parse_env_value(&env_content.stdout, "DB_PASSWORD") {
            /* Parsear DATABASE_URL del compose para extraer usuario y base de datos */
            let compose_content = ssh
                .execute(&format!(
                    "cat {service_dir}/docker-compose.yml 2>/dev/null || echo ''"
                ))
                .await?;
            let (user, db) = extract_user_db_from_compose(&compose_content.stdout)
                .unwrap_or_else(|| ("glory_app".to_string(), "glory".to_string()));
            (pw, user, db)
        } else {
            return Err(CoolifyError::Validation(
            "SERVICE_PASSWORD_POSTGRES no existe en .env remoto y tampoco se encontro DB_PASSWORD"
                .into(),
        ));
        };

    let postgres_container = format!("postgres-{stack_uuid}");

    /* [incident-2026-07-02] E20: Verificar que la base de datos objetivo existe en el
     * contenedor postgres antes de intentar ALTER USER. Si no existe, algo cambió
     * las credenciales del compose (Coolify regeneró, edición manual, etc.) y
     * continuar causaría que la app corra migraciones sobre una DB vacía nueva. */
    let check_db_cmd = format!(
        "docker exec {postgres_container} psql -U {db_user} -d postgres -tAc \
         \"SELECT 1 FROM pg_database WHERE datname = '{db_name}'\" 2>/dev/null || echo '0'"
    );
    let db_exists = ssh.execute(&check_db_cmd).await?;
    let db_exists_result = db_exists.stdout.trim();
    if db_exists_result != "1" {
        /* La DB no existe — verificar si existe otra DB con datos para detectar drift */
        let list_dbs_cmd = format!(
            "docker exec {postgres_container} psql -U {db_user} -d postgres -tAc \
             \"SELECT datname || ':' || pg_database_size(datname) FROM pg_database \
             WHERE datistemplate = false AND datname != 'postgres' ORDER BY pg_database_size(datname) DESC\" 2>/dev/null || true"
        );
        let dbs = ssh.execute(&list_dbs_cmd).await?;
        return Err(CoolifyError::Validation(format!(
            "E20: Base de datos '{}' no existe en el contenedor postgres-{}. \
             Credenciales del compose: user={}, db={}. \
             Bases existentes: {}. \
             Posible causa: Coolify regeneró el compose con credenciales distintas \
             (mecanismo que causó pérdida de datos en glory-rest el 2026-07-01). \
             NO se ejecutará ALTER USER para evitar crear una DB nueva vacía. \
             Solución: restaurar el compose original con las credenciales correctas.",
            db_name,
            stack_uuid,
            db_user,
            db_name,
            dbs.stdout.trim().replace('\n', ", ")
        )));
    }
    tracing::info!(
        "E20: Base de datos '{}' verificada en postgres-{}",
        db_name,
        stack_uuid
    );

    let sql = format!(
        "ALTER USER {} WITH PASSWORD '{}';",
        db_user,
        escape_sql_string(&password)
    );
    let encoded_sql = base64_encode(sql.as_bytes());
    let alter_cmd = format!(
        "echo {encoded_sql} | base64 -d | docker exec -i {postgres_container} psql -U {db_user} -d {db_name}"
    );
    let alter_result = ssh.execute(&alter_cmd).await?;
    if alter_result.exit_code != 0 || !alter_result.stdout.contains("ALTER ROLE") {
        return Err(CoolifyError::Validation(format!(
            "No se pudo alinear password de Postgres: {}{}",
            alter_result.stdout.trim(),
            alter_result.stderr.trim()
        )));
    }

    let compose_file = format!("{service_dir}/docker-compose.yml");
    let sed_cmd = format!("sed -i 's|@postgres:|@{postgres_container}:|g' {compose_file}");
    let sed_result = ssh.execute(&sed_cmd).await?;
    if sed_result.exit_code != 0 {
        return Err(CoolifyError::Validation(format!(
            "No se pudo corregir DATABASE_URL en compose: {}",
            sed_result.stderr.trim()
        )));
    }

    /* [303A-7] Sincronizar password en DATABASE_URL con SERVICE_PASSWORD_POSTGRES.
     * Coolify puede regenerar SERVICE_PASSWORD_POSTGRES en .env durante un resync;
     * el ALTER USER de arriba sincroniza Postgres, pero DATABASE_URL en compose
     * sigue teniendo el password viejo hardcodeado → la app arranca con 28P01.
     * Reemplazamos el password en DATABASE_URL para que coincida. */
    let escaped_password = escape_sed_replacement(&password);
    /* sed 's|\(DATABASE_URL:.*://[^:]*:\)[^@]*\(@.*\)|\1{password}\2|' */
    let db_url_sed = format!(
        "sed -i 's|\\(DATABASE_URL:.*://[^:]*:\\)[^@]*\\(@.*\\)|\\1{escaped_password}\\2|' {compose_file}"
    );
    let db_url_result = ssh.execute(&db_url_sed).await?;
    if db_url_result.exit_code != 0 {
        return Err(CoolifyError::Validation(format!(
            "No se pudo actualizar password en DATABASE_URL: {}",
            db_url_result.stderr.trim()
        )));
    }
    println!("      DATABASE_URL sincronizado con SERVICE_PASSWORD_POSTGRES.");

    Ok(())
}

pub(crate) fn parse_env_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(&format!("{key}=")) {
            let value = rest.trim_matches('"').trim_matches('\'').to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

pub(crate) fn escape_sql_string(value: &str) -> String {
    value.replace('\'', "''")
}

pub(crate) fn escape_sed_replacement(value: &str) -> String {
    /* Escapa caracteres especiales de sed en la cadena de reemplazo:
     * \, &, y el separador | (usado en nuestros comandos sed). */
    value
        .replace('\\', "\\\\")
        .replace('&', "\\&")
        .replace('|', "\\|")
}

pub(crate) fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;

    base64::engine::general_purpose::STANDARD.encode(data)
}
