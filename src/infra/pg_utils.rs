/*
 * Utilidades compartidas para operaciones PostgreSQL via Docker exec.
 * Extraídas de run_sql, db_check, db_migrate, restore_client para eliminar duplicación.
 */

use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;

use base64::Engine as _;

/// Extrae usuario y nombre de base de datos de una DATABASE_URL de PostgreSQL.
pub fn parse_pg_credentials(database_url: &str) -> std::result::Result<(String, String), CoolifyError> {
    let without_scheme = database_url
        .strip_prefix("postgres://")
        .or_else(|| database_url.strip_prefix("postgresql://"))
        .ok_or_else(|| CoolifyError::Validation("DATABASE_URL no es una URL postgres válida".into()))?;

    let at_pos = without_scheme
        .find('@')
        .ok_or_else(|| CoolifyError::Validation("DATABASE_URL: falta @ en la URL".into()))?;

    let user_part = &without_scheme[..at_pos];
    let user = user_part
        .split(':')
        .next()
        .unwrap_or(user_part)
        .to_string();

    let after_at = &without_scheme[at_pos + 1..];
    let path_part = after_at.split('?').next().unwrap_or(after_at);
    let db_name = path_part
        .rsplit('/')
        .next()
        .unwrap_or("postgres")
        .to_string();

    Ok((user, db_name))
}

/// Ejecuta una query SQL contra el contenedor PostgreSQL via docker exec.
/// Retorna stdout (usar -t -A para formato limpio).
pub async fn run_pg_query(
    ssh: &SshClient,
    pg_container: &str,
    db_user: &str,
    db_name: &str,
    sql: &str,
) -> std::result::Result<String, CoolifyError> {
    let sql_b64 = base64::engine::general_purpose::STANDARD.encode(sql.as_bytes());
    let cmd = format!(
        "echo '{}' | base64 -d | docker exec -i {} psql -U {} -d {} -t -A 2>&1",
        sql_b64, pg_container, db_user, db_name
    );
    let result = ssh.execute(&cmd).await?;
    Ok(result.stdout)
}

/// Obtiene las credenciales de PostgreSQL del contenedor app de un stack.
pub async fn get_pg_credentials(
    ssh: &SshClient,
    stack_uuid: &str,
) -> std::result::Result<(String, String, String, String), CoolifyError> {
    /* Retorna (pg_container_id, db_user, db_name, database_url) */
    let pg_container = docker::find_postgres_container(ssh, stack_uuid).await?;
    let app_container = docker::find_app_container(ssh, stack_uuid).await?;

    let db_url_result = docker::docker_exec(ssh, &app_container, "printenv DATABASE_URL").await?;
    let database_url = db_url_result.stdout.trim().to_string();
    let (db_user, db_name) = parse_pg_credentials(&database_url)?;

    Ok((pg_container, db_user, db_name, database_url))
}

/// Valida que un nombre de tabla solo contenga caracteres seguros [a-z0-9_].
pub fn validate_table_name(name: &str) -> std::result::Result<(), CoolifyError> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
        return Err(CoolifyError::Validation(format!(
            "Nombre de tabla inválido: '{}' (solo lowercase, dígitos y _)",
            name
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pg_credentials() {
        let (user, db) = parse_pg_credentials(
            "postgres://rust_app:BHZKfizTka82I2dQxpRefEQdSPPXrQVg@postgres-do8k:5432/rust_db?sslmode=disable",
        )
        .unwrap();
        assert_eq!(user, "rust_app");
        assert_eq!(db, "rust_db");
    }

    #[test]
    fn test_parse_pg_credentials_postgresql_scheme() {
        let (user, db) = parse_pg_credentials(
            "postgresql://app:pass@localhost/mydb",
        )
        .unwrap();
        assert_eq!(user, "app");
        assert_eq!(db, "mydb");
    }

    #[test]
    fn test_validate_table_name() {
        assert!(validate_table_name("hosting_subscriptions").is_ok());
        assert!(validate_table_name("billing_items").is_ok());
        assert!(validate_table_name("").is_err());
        assert!(validate_table_name("DROP TABLE").is_err());
        assert!(validate_table_name("table;DELETE").is_err());
        assert!(validate_table_name("Table_Name").is_err()); /* uppercase */
    }
}
