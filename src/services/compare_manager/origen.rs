/* Split 119A-5 de compare_manager.rs — resolución del lado vivo (PG/MariaDB).
 * Código verbatim del original; solo cambia visibilidad de ayudantes compartidos. */

use super::tipos::SideCreds;
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::pg_utils;
use crate::infra::ssh_client::SshClient;
use crate::services::compare::schema::{
    discover_mariadb, discover_postgres, DbEngine, SchemaModel,
};

/// Resuelve credenciales de la BD viva de un sitio.
pub(super) async fn resolve_live_creds(
    ssh: &SshClient,
    stack_uuid: &str,
) -> std::result::Result<SideCreds, CoolifyError> {
    /* Primero probar PostgreSQL (stacks Rust/kamples) */
    match docker::find_postgres_container(ssh, stack_uuid).await {
        Ok(pg_container) => {
            let (db_user, db_name) = {
                let app = docker::find_app_container(ssh, stack_uuid).await?;
                let url = docker::docker_exec(ssh, &app, "printenv DATABASE_URL")
                    .await?
                    .stdout;
                let url = url.trim().to_string();
                if url.is_empty() {
                    /* Kamples usa KAMPLES_PG_* */
                    let (db, user, pass) =
                        crate::services::database_manager::resolve_postgres_credentials(ssh, &app)
                            .await?;
                    let _ = pass;
                    (user, db)
                } else {
                    pg_utils::parse_pg_credentials(&url)?
                }
            };
            Ok(SideCreds {
                engine: DbEngine::Postgres,
                container: pg_container,
                db_user,
                db_name,
                db_password: None,
            })
        }
        Err(_) => {
            /* Fallback: MariaDB/WordPress */
            let wp = docker::find_wordpress_container(ssh, stack_uuid).await?;
            let (db_name, db_user, db_password) =
                crate::services::database_manager::resolve_wordpress_credentials(ssh, &wp).await?;
            Ok(SideCreds {
                engine: DbEngine::MariaDb,
                container: docker::find_mariadb_container(ssh, stack_uuid).await?,
                db_user,
                db_name,
                db_password: Some(db_password),
            })
        }
    }
}

/// Descubre el esquema de un lado.
pub(super) async fn discover_schema(
    ssh: &SshClient,
    creds: &SideCreds,
) -> std::result::Result<SchemaModel, CoolifyError> {
    match creds.engine {
        DbEngine::Postgres => {
            discover_postgres(ssh, &creds.container, &creds.db_user, &creds.db_name).await
        }
        DbEngine::MariaDb => {
            let pw = creds
                .db_password
                .as_ref()
                .ok_or_else(|| CoolifyError::Validation("MariaDB sin password".into()))?;
            discover_mariadb(ssh, &creds.container, &creds.db_name, &creds.db_user, pw).await
        }
    }
}
