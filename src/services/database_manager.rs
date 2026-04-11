/*
 * DatabaseManager — import/export de bases de datos WordPress.
 * Equivale a WordPress/DatabaseManager.psm1.
 */

use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;

use secrecy::ExposeSecret;
use secrecy::SecretString;
use std::path::Path;

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub async fn resolve_wordpress_credentials(
    ssh: &SshClient,
    app_container: &str,
) -> std::result::Result<(String, String, SecretString), CoolifyError> {
    let db_name = docker::docker_exec(ssh, app_container, "printenv WORDPRESS_DB_NAME")
        .await?
        .stdout
        .trim()
        .to_string();
    let db_user = docker::docker_exec(ssh, app_container, "printenv WORDPRESS_DB_USER")
        .await?
        .stdout
        .trim()
        .to_string();
    let db_password = docker::docker_exec(ssh, app_container, "printenv WORDPRESS_DB_PASSWORD")
        .await?
        .stdout
        .trim()
        .to_string();

    if db_name.is_empty() || db_user.is_empty() || db_password.is_empty() {
        return Err(CoolifyError::Validation(
            "Credenciales MariaDB no disponibles en el contenedor WordPress".to_string(),
        ));
    }

    Ok((db_name, db_user, SecretString::from(db_password)))
}

pub async fn resolve_postgres_credentials(
    ssh: &SshClient,
    app_container: &str,
) -> std::result::Result<(String, String, SecretString), CoolifyError> {
    /* [114A-18] Intenta primero KAMPLES_PG_* (Kamples template), luego DATABASE_URL (Rust template).
     * DATABASE_URL tiene formato: postgres://user:password@host:port/dbname */
    let db_name = docker::docker_exec(ssh, app_container, "printenv KAMPLES_PG_DBNAME")
        .await?
        .stdout
        .trim()
        .to_string();
    let db_user = docker::docker_exec(ssh, app_container, "printenv KAMPLES_PG_USER")
        .await?
        .stdout
        .trim()
        .to_string();
    let db_password = docker::docker_exec(ssh, app_container, "printenv KAMPLES_PG_PASSWORD")
        .await?
        .stdout
        .trim()
        .to_string();

    if !db_name.is_empty() && !db_user.is_empty() && !db_password.is_empty() {
        return Ok((db_name, db_user, SecretString::from(db_password)));
    }

    /* Fallback: parsear DATABASE_URL (Rust template usa postgres://user:pass@host:port/db) */
    let database_url = docker::docker_exec(ssh, app_container, "printenv DATABASE_URL")
        .await?
        .stdout
        .trim()
        .to_string();

    if let Some(creds) = parse_database_url(&database_url) {
        return Ok(creds);
    }

    Err(CoolifyError::Validation(
        "Credenciales PostgreSQL no disponibles: ni KAMPLES_PG_* ni DATABASE_URL encontrados en el contenedor".to_string(),
    ))
}

/* [114A-18] Parsea postgres://user:password@host:port/dbname y extrae (dbname, user, password).
 * Soporta URLs con y sin puerto. No usa crate externo para minimizar dependencias. */
fn parse_database_url(url: &str) -> Option<(String, String, SecretString)> {
    let rest = url.strip_prefix("postgres://").or_else(|| url.strip_prefix("postgresql://"))?;
    let (userinfo, after_at) = rest.split_once('@')?;
    let (user, password) = userinfo.split_once(':')?;
    /* after_at = host:port/dbname o host/dbname */
    let dbname = after_at.split('/').nth(1)?;
    let dbname = dbname.split('?').next().unwrap_or(dbname);

    if user.is_empty() || password.is_empty() || dbname.is_empty() {
        return None;
    }

    Some((dbname.to_string(), user.to_string(), SecretString::from(password.to_string())))
}

/// Importa un archivo SQL en la base de datos WordPress.
pub async fn import_database(
    ssh: &SshClient,
    mariadb_container: &str,
    sql_file: &Path,
    db_name: &str,
    db_user: &str,
    db_password: &SecretString,
) -> std::result::Result<(), CoolifyError> {
    tracing::info!("Importando base de datos desde {}", sql_file.display());

    /* Copiar archivo SQL al contenedor */
    let remote_path = "/tmp/import.sql";
    docker::copy_to_container(ssh, sql_file, mariadb_container, remote_path).await?;

    /* Ejecutar importacion */
    let cmd = format!(
        "mariadb -u {user} -p'{password}' {db} < {file}",
        user = db_user,
        password = db_password.expose_secret(),
        db = db_name,
        file = remote_path
    );

    let result = docker::docker_exec(ssh, mariadb_container, &cmd).await?;

    /* Limpiar archivo temporal */
    let _ = docker::docker_exec(ssh, mariadb_container, &format!("rm -f {remote_path}")).await;

    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("Error importando SQL: {}", result.stderr),
        });
    }

    tracing::info!("Base de datos importada exitosamente");
    Ok(())
}

/// Exporta la base de datos WordPress a un archivo SQL local.
pub async fn export_database(
    ssh: &SshClient,
    mariadb_container: &str,
    db_name: &str,
    db_user: &str,
    db_password: &SecretString,
    output_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    tracing::info!("Exportando base de datos a {}", output_path.display());

    let remote_path = "/tmp/export.sql";

    let cmd = format!(
        "mysqldump -u {user} -p'{password}' {db} > {file}",
        user = db_user,
        password = db_password.expose_secret(),
        db = db_name,
        file = remote_path
    );

    let result = docker::docker_exec(ssh, mariadb_container, &cmd).await?;

    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("Error exportando SQL: {}", result.stderr),
        });
    }

    /* Descargar archivo del contenedor */
    docker::copy_from_container(ssh, mariadb_container, remote_path, output_path).await?;

    /* Limpiar */
    let _ = docker::docker_exec(ssh, mariadb_container, &format!("rm -f {remote_path}")).await;

    tracing::info!("Base de datos exportada a {}", output_path.display());
    Ok(())
}

/// Exporta una base PostgreSQL a un archivo SQL local.
pub async fn export_postgres_database(
    ssh: &SshClient,
    postgres_container: &str,
    db_name: &str,
    db_user: &str,
    db_password: &SecretString,
    output_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    tracing::info!("Exportando base PostgreSQL a {}", output_path.display());

    let remote_path = "/tmp/export_pg.sql";
    let cmd = format!(
        "export PGPASSWORD={password} && pg_dump -U {user} {db} > {file}",
        password = shell_quote(db_password.expose_secret()),
        user = shell_quote(db_user),
        db = shell_quote(db_name),
        file = remote_path,
    );

    let result = docker::docker_exec(ssh, postgres_container, &cmd).await?;
    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("Error exportando PostgreSQL: {}", result.stderr),
        });
    }

    docker::copy_from_container(ssh, postgres_container, remote_path, output_path).await?;
    let _ = docker::docker_exec(ssh, postgres_container, &format!("rm -f {remote_path}")).await;
    Ok(())
}

/// Importa un dump PostgreSQL desde un archivo SQL local.
pub async fn import_postgres_database(
    ssh: &SshClient,
    postgres_container: &str,
    sql_file: &Path,
    db_name: &str,
    db_user: &str,
    db_password: &SecretString,
) -> std::result::Result<(), CoolifyError> {
    tracing::info!("Importando base PostgreSQL desde {}", sql_file.display());

    let remote_path = "/tmp/import_pg.sql";
    docker::copy_to_container(ssh, sql_file, postgres_container, remote_path).await?;

    let cmd = format!(
        "export PGPASSWORD={password} && psql -U {user} {db} < {file}",
        password = shell_quote(db_password.expose_secret()),
        user = shell_quote(db_user),
        db = shell_quote(db_name),
        file = remote_path,
    );

    let result = docker::docker_exec(ssh, postgres_container, &cmd).await?;
    let _ = docker::docker_exec(ssh, postgres_container, &format!("rm -f {remote_path}")).await;

    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("Error importando PostgreSQL: {}", result.stderr),
        });
    }

    Ok(())
}

/* [DIRECT-TRANSFER] Exporta MariaDB dejando el SQL en el host VPS1 (no descarga a local).
 * Usa nice/ionice para minimizar impacto en el sitio en produccion. */
pub async fn export_database_to_host(
    ssh: &SshClient,
    mariadb_container: &str,
    db_name: &str,
    db_user: &str,
    db_password: &SecretString,
    host_output_path: &str,
) -> std::result::Result<(), CoolifyError> {
    tracing::info!("Exportando base de datos a VPS1:{}", host_output_path);

    let container_path = "/tmp/cm_export.sql";
    let cmd = format!(
        "nice -n 19 ionice -c 3 mysqldump -u {user} -p'{password}' {db} > {file}",
        user = db_user,
        password = db_password.expose_secret(),
        db = db_name,
        file = container_path
    );

    let result = docker::docker_exec(ssh, mariadb_container, &cmd).await?;
    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("Error exportando SQL (server-side): {}", result.stderr),
        });
    }

    docker::copy_from_container_to_host(ssh, mariadb_container, container_path, host_output_path)
        .await?;
    let _ = docker::docker_exec(ssh, mariadb_container, &format!("rm -f {container_path}")).await;

    tracing::info!("Base de datos exportada a VPS1:{}", host_output_path);
    Ok(())
}

/* [DIRECT-TRANSFER] Exporta PostgreSQL dejando el SQL en el host VPS1 (no descarga a local). */
pub async fn export_postgres_database_to_host(
    ssh: &SshClient,
    postgres_container: &str,
    db_name: &str,
    db_user: &str,
    db_password: &SecretString,
    host_output_path: &str,
) -> std::result::Result<(), CoolifyError> {
    tracing::info!("Exportando PostgreSQL a VPS1:{}", host_output_path);

    let container_path = "/tmp/cm_export_pg.sql";
    let cmd = format!(
        "nice -n 19 ionice -c 3 pg_dump -U {user} {db} > {file}",
        user = shell_quote(db_user),
        db = shell_quote(db_name),
        file = container_path,
    );
    /* pg_dump necesita PGPASSWORD */
    let full_cmd = format!(
        "export PGPASSWORD={} && {}",
        shell_quote(db_password.expose_secret()),
        cmd
    );

    let result = docker::docker_exec(ssh, postgres_container, &full_cmd).await?;
    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("Error exportando PostgreSQL (server-side): {}", result.stderr),
        });
    }

    docker::copy_from_container_to_host(ssh, postgres_container, container_path, host_output_path)
        .await?;
    let _ =
        docker::docker_exec(ssh, postgres_container, &format!("rm -f {container_path}")).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn parse_database_url_standard() {
        let (db, user, pass) = parse_database_url("postgres://myuser:mypass@localhost:5432/mydb").unwrap();
        assert_eq!(db, "mydb");
        assert_eq!(user, "myuser");
        assert_eq!(pass.expose_secret(), "mypass");
    }

    #[test]
    fn parse_database_url_no_port() {
        let (db, user, pass) = parse_database_url("postgres://admin:secret@db-host/appdb").unwrap();
        assert_eq!(db, "appdb");
        assert_eq!(user, "admin");
        assert_eq!(pass.expose_secret(), "secret");
    }

    #[test]
    fn parse_database_url_postgresql_scheme() {
        let (db, user, pass) = parse_database_url("postgresql://u:p@h:5432/d").unwrap();
        assert_eq!(db, "d");
        assert_eq!(user, "u");
        assert_eq!(pass.expose_secret(), "p");
    }

    #[test]
    fn parse_database_url_with_query_params() {
        let (db, _, _) = parse_database_url("postgres://u:p@h:5432/mydb?sslmode=require").unwrap();
        assert_eq!(db, "mydb");
    }

    #[test]
    fn parse_database_url_invalid_scheme() {
        assert!(parse_database_url("mysql://u:p@h/db").is_none());
    }

    #[test]
    fn parse_database_url_empty() {
        assert!(parse_database_url("").is_none());
    }

    #[test]
    fn parse_database_url_no_password() {
        assert!(parse_database_url("postgres://user@host/db").is_none());
    }

    #[test]
    fn parse_database_url_special_chars_in_password() {
        let (_, _, pass) = parse_database_url("postgres://u:p%40ss@h/db").unwrap();
        assert_eq!(pass.expose_secret(), "p%40ss");
    }
}
