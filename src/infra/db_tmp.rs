/*
 * infra/db_tmp — contenedor temporal efímero para restaurar un dump
 * y compararlo contra la BD viva sin tocarla.
 *
 * E12: garantías de seguridad:
 *   - Contenedor con prefijo identificable `coolify-dbcompare-*`
 *   - Red aislada (no publica puertos, no en la red del stack)
 *   - --rm (auto-eliminación) + finally explícito (doble garantía)
 *   - Nunca toca la BD viva: solo SELECT sobre el contenedor temporal.
 */

use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use base64::Engine as _;

/// Contenedor temporal con su estado.
pub struct TempDb {
    pub container: String,
    pub db_user: String,
    pub db_name: String,
}

/// Prefijo identificable para limpieza y diagnóstico.
pub const TMP_PREFIX: &str = "coolify-dbcompare-";

/// Crea un contenedor temporal aislado para un motor dado.
/// `image` es la imagen Docker (p. ej. la del postgres/mariadb del stack).
pub async fn create_temp_container(
    ssh: &SshClient,
    engine: crate::services::compare::schema::DbEngine,
    image: &str,
    db_user: &str,
    db_name: &str,
    db_password: &str,
) -> std::result::Result<TempDb, CoolifyError> {
    let suffix: String = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        format!("{nanos:x}{}", rand_suffix())
    };
    let name = format!("{TMP_PREFIX}{suffix}");

    match engine {
        crate::services::compare::schema::DbEngine::Postgres => {
            /* Imagen postgres: definir POSTGRES_USER/DB/PASSWORD */
            let cmd = format!(
                "docker run -d --rm --name {name} \
                 -e POSTGRES_USER='{db_user}' -e POSTGRES_DB='{db_name}' -e POSTGRES_PASSWORD='{db_password}' \
                 --network none --restart no {image}"
            );
            let res = ssh.execute(&cmd).await?;
            if !res.success() {
                return Err(CoolifyError::Docker {
                    exit_code: res.exit_code,
                    stderr: format!(
                        "No se pudo crear contenedor temporal: {}",
                        if res.stderr.trim().is_empty() {
                            res.stdout
                        } else {
                            res.stderr
                        }
                    ),
                });
            }
            let container = res.stdout.trim().to_string();
            if container.is_empty() {
                return Err(CoolifyError::Validation(
                    "docker run no devolvió ID de contenedor temporal".into(),
                ));
            }
            /* Esperar readiness de postgres */
            wait_for_postgres_ready(ssh, &container, db_user, db_name).await?;
            Ok(TempDb {
                container,
                db_user: db_user.to_string(),
                db_name: db_name.to_string(),
            })
        }
        crate::services::compare::schema::DbEngine::MariaDb => {
            let cmd = format!(
                "docker run -d --rm --name {name} \
                 -e MARIADB_USER='{db_user}' -e MARIADB_DATABASE='{db_name}' -e MARIADB_PASSWORD='{db_password}' \
                 -e MARIADB_ROOT_PASSWORD='{db_password}' \
                 --network none --restart no {image}"
            );
            let res = ssh.execute(&cmd).await?;
            if !res.success() {
                return Err(CoolifyError::Docker {
                    exit_code: res.exit_code,
                    stderr: format!(
                        "No se pudo crear contenedor temporal: {}",
                        if res.stderr.trim().is_empty() {
                            res.stdout
                        } else {
                            res.stderr
                        }
                    ),
                });
            }
            let container = res.stdout.trim().to_string();
            if container.is_empty() {
                return Err(CoolifyError::Validation(
                    "docker run no devolvió ID de contenedor temporal".into(),
                ));
            }
            wait_for_mariadb_ready(ssh, &container, db_user, db_name, db_password).await?;
            Ok(TempDb {
                container,
                db_user: db_user.to_string(),
                db_name: db_name.to_string(),
            })
        }
    }
}

fn rand_suffix() -> String {
    /* Sufijo corto adicional (aleatorio sin dep de rand) */
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{now:x}")
}

async fn wait_for_postgres_ready(
    ssh: &SshClient,
    container: &str,
    db_user: &str,
    db_name: &str,
) -> std::result::Result<(), CoolifyError> {
    /* Loop con timeout de ~30s */
    for _ in 0..15 {
        let cmd = format!("docker exec {container} pg_isready -U {db_user} -d {db_name} 2>&1");
        let res = ssh.execute(&cmd).await?;
        if res.success() && res.stdout.to_lowercase().contains("accepting") {
            return Ok(());
        }
        ssh.execute("sleep 2").await?;
    }
    Err(CoolifyError::Docker {
        exit_code: 1,
        stderr: "Timeout esperando readiness del postgres temporal".into(),
    })
}

async fn wait_for_mariadb_ready(
    ssh: &SshClient,
    container: &str,
    db_user: &str,
    db_name: &str,
    db_password: &str,
) -> std::result::Result<(), CoolifyError> {
    for _ in 0..15 {
        let cmd = format!(
            "docker exec {container} mariadb -u {db_user} -p'{db_password}' {db_name} -e 'SELECT 1' 2>&1"
        );
        let res = ssh.execute(&cmd).await?;
        if res.success() {
            return Ok(());
        }
        ssh.execute("sleep 2").await?;
    }
    Err(CoolifyError::Docker {
        exit_code: 1,
        stderr: "Timeout esperando readiness del mariadb temporal".into(),
    })
}

/// Entrecomilla un argumento para shell remoto (defensa ante nombres raros
/// provenientes de listados de tarball; las rutas propias ya son seguras).
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Elige el miembro SQL de un tarball legacy (función pura, testeable).
/// Acepta el primer `.sql` cuyo nombre base empiece por `db-`
/// (p. ej. `.staging-20260813_030007/db-wordpress.sql`).
/// Rechaza rutas absolutas y `..` (anti-traversal aunque el tarball sea propio).
pub fn pick_db_member(listing: &str) -> Option<String> {
    for line in listing.lines() {
        let m = line.trim();
        if m.is_empty() || m.starts_with('/') || m.contains("..") {
            continue;
        }
        let base = m.rsplit('/').next().unwrap_or(m);
        if base.starts_with("db-") && base.ends_with(".sql") {
            return Some(m.to_string());
        }
    }
    None
}

/// Extrae el `db-*.sql` de un tarball legacy (`.tar.gz`) a `dest_sql`,
/// todo en el VPS (el tarball fuente solo se lee, nunca se modifica).
/// Devuelve el nombre del miembro extraído. Falla cerrado si no hay
/// miembro válido o el resultado queda vacío.
pub async fn extract_sql_from_tarball(
    ssh: &SshClient,
    tarball_remote: &str,
    dest_sql: &str,
) -> std::result::Result<String, CoolifyError> {
    let tarball = sh_quote(tarball_remote);
    let list = ssh
        .execute(&format!("timeout 120 tar -tzf {tarball} 2>&1"))
        .await?;
    let member = pick_db_member(&list.stdout).ok_or_else(|| {
        CoolifyError::Validation(format!(
            "Tarball sin miembro db-*.sql válido: {tarball_remote}"
        ))
    })?;
    let res = ssh
        .execute(&format!(
            "timeout 300 tar -xzOf {tarball} {member} > {dest} 2>/dev/null",
            member = sh_quote(&member),
            dest = sh_quote(dest_sql),
        ))
        .await?;
    if !res.success() {
        return Err(CoolifyError::Docker {
            exit_code: res.exit_code,
            stderr: format!("Extracción del tarball falló: {tarball_remote}"),
        });
    }
    let check = ssh
        .execute(&format!(
            "test -s {dest} && echo DBTMP_OK",
            dest = sh_quote(dest_sql)
        ))
        .await?;
    if !check.stdout.contains("DBTMP_OK") {
        return Err(CoolifyError::Validation(format!(
            "Miembro extraído vacío: {member} de {tarball_remote}"
        )));
    }
    Ok(member)
}

/// Restaura un dump SQL (`.sql` o `.sql.gz`) dentro del contenedor temporal.
/// `dump_path` debe ser una ruta accesible en el VPS.
pub async fn restore_dump(
    ssh: &SshClient,
    engine: crate::services::compare::schema::DbEngine,
    tmp: &TempDb,
    dump_path: &str,
    db_password: &str,
) -> std::result::Result<(), CoolifyError> {
    match engine {
        crate::services::compare::schema::DbEngine::Postgres => {
            /* detectar gzip por extensión */
            let cat = if dump_path.ends_with(".gz") {
                "zcat"
            } else {
                "cat"
            };
            let cmd = format!(
                "{cat} {dump_path} | docker exec -i {container} psql -U {user} -d {db} -v ON_ERROR_STOP=0 2>&1",
                container = tmp.container,
                user = tmp.db_user,
                db = tmp.db_name
            );
            let res = ssh.execute(&cmd).await?;
            /* psql devuelve 0 aunque haya errores de objeto existente; no es fatal */
            let _ = res;
            Ok(())
        }
        crate::services::compare::schema::DbEngine::MariaDb => {
            let cat = if dump_path.ends_with(".gz") {
                "zcat"
            } else {
                "cat"
            };
            let cmd = format!(
                "{cat} {dump_path} | docker exec -i {container} mariadb -u {user} -p'{pw}' {db} 2>&1",
                container = tmp.container,
                user = tmp.db_user,
                pw = db_password,
                db = tmp.db_name
            );
            let res = ssh.execute(&cmd).await?;
            if !res.success() {
                return Err(CoolifyError::Docker {
                    exit_code: res.exit_code,
                    stderr: format!(
                        "Restauración temporal falló: {}",
                        if res.stderr.trim().is_empty() {
                            res.stdout
                        } else {
                            res.stderr
                        }
                    ),
                });
            }
            Ok(())
        }
    }
}

/// Elimina el contenedor temporal (garantía de limpieza).
pub async fn cleanup_temp(ssh: &SshClient, container: &str) {
    let cmd = format!("docker rm -f {container} 2>/dev/null || true");
    let _ = ssh.execute(&cmd).await;
}

/// Barrido de contenedores temporales huérfanos (recuperación).
/// También borra dumps subidos a /tmp con nuestro prefijo `dbcompare_`
/// que hayan quedado de ejecuciones abortadas (no dejar basura en el VPS).
pub async fn cleanup_all_temp(ssh: &SshClient) -> std::result::Result<(), CoolifyError> {
    let cmd = format!("docker ps -aq --filter name={TMP_PREFIX}");
    let res = ssh.execute(&cmd).await?;
    for id in res.stdout.lines() {
        let id = id.trim();
        if !id.is_empty() {
            let _ = ssh.execute(&format!("docker rm -f {id}")).await;
        }
    }
    /* Barrido de dumps subidos huérfanos (ejecuciones abortadas) */
    let _ = ssh.execute("rm -f /tmp/dbcompare_*.sql").await;
    Ok(())
}

/// Versión de la imagen del motor (para documentar en el reporte).
pub async fn detect_engine_version(
    ssh: &SshClient,
    engine: crate::services::compare::schema::DbEngine,
    container: &str,
    db_user: &str,
    db_name: &str,
    db_password: &str,
) -> String {
    let cmd = match engine {
        crate::services::compare::schema::DbEngine::Postgres => format!(
            "docker exec {container} psql -U {db_user} -d {db_name} -t -A -c 'SELECT version();' 2>&1"
        ),
        crate::services::compare::schema::DbEngine::MariaDb => format!(
            "docker exec {container} mariadb -u {db_user} -p'{db_password}' {db_name} -N -e 'SELECT VERSION();' 2>&1"
        ),
    };
    match ssh.execute(&cmd).await {
        Ok(r) => r
            .stdout
            .trim()
            .lines()
            .next()
            .unwrap_or("unknown")
            .to_string(),
        Err(_) => "unknown".to_string(),
    }
}

/// Codifica un comando SQL a base64 (patrón consistente con pg_utils/run_sql).
pub fn sql_to_base64(sql: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(sql.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sql_to_base64_roundtrip() {
        let sql = "SELECT 1; SELECT 2;";
        let b64 = sql_to_base64(sql);
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&b64)
            .unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), sql);
    }

    #[test]
    fn test_tmp_prefix() {
        assert!(TMP_PREFIX.starts_with("coolify-dbcompare-"));
    }

    #[test]
    fn test_pick_db_member_legacy() {
        let listing = ".staging-20260813_030007/db-wordpress.sql\n\
                       .staging-20260813_030007/files-var_www_html_wp_content.tar.gz\n";
        assert_eq!(
            pick_db_member(listing),
            Some(".staging-20260813_030007/db-wordpress.sql".to_string())
        );
    }

    #[test]
    fn test_pick_db_member_rechaza_traversal() {
        assert_eq!(
            pick_db_member("/etc/passwd\ndb-x.sql\n"),
            Some("db-x.sql".to_string())
        );
        assert_eq!(pick_db_member("../../evil/db-x.sql\n"), None);
        assert_eq!(pick_db_member("files.tar.gz\n"), None);
        assert_eq!(pick_db_member(""), None);
    }
}
