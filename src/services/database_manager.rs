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

    if db_name.is_empty() || db_user.is_empty() || db_password.is_empty() {
        return Err(CoolifyError::Validation(
            "Credenciales PostgreSQL no disponibles en el contenedor de aplicacion".to_string(),
        ));
    }

    Ok((db_name, db_user, SecretString::from(db_password)))
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
