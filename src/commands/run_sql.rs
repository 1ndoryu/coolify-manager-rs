/*
 * Comando: run-sql
 * Ejecuta SQL arbitrario contra el contenedor PostgreSQL de un sitio.
 * Soporta --query (inline) y --file (archivo local .sql).
 * Con --dry-run, envuelve en BEGIN/ROLLBACK.
 *
 * Flujo seguro: SQL -> base64 -> docker exec psql -> resultado.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::pg_utils;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;

use base64::Engine as _;
use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    query: Option<&str>,
    file: Option<&Path>,
    dry_run: bool,
) -> std::result::Result<(), CoolifyError> {
    if query.is_none() && file.is_none() {
        return Err(CoolifyError::Validation(
            "Especifica --query o --file".into(),
        ));
    }

    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    let stack_uuid = site.stack_uuid.as_deref().unwrap();
    let target_config = settings.resolve_site_target(site)?;

    let mut ssh = SshClient::from_vps(&target_config.vps);
    ssh.connect().await?;

    let (pg_container, db_user, db_name, _) =
        pg_utils::get_pg_credentials(&ssh, stack_uuid).await?;

    /* Construir SQL final */
    let raw_sql = if let Some(q) = query {
        q.to_string()
    } else {
        let path = file.unwrap();
        std::fs::read_to_string(path).map_err(|e| {
            CoolifyError::Validation(format!("Error leyendo {}: {}", path.display(), e))
        })?
    };

    let final_sql = if dry_run {
        format!("BEGIN;\n{}\nROLLBACK;", raw_sql)
    } else {
        raw_sql
    };

    /* Codificar en base64 y ejecutar via psql */
    let sql_b64 = base64::engine::general_purpose::STANDARD.encode(final_sql.as_bytes());

    tracing::info!(
        "Ejecutando SQL ({} bytes, dry_run={}) en postgres container {}",
        final_sql.len(),
        dry_run,
        pg_container
    );

    let cmd = format!(
        "echo '{}' | base64 -d | docker exec -i {} psql -U {} -d {} 2>&1",
        sql_b64, pg_container, db_user, db_name
    );

    let result = ssh.execute(&cmd).await?;

    if !result.stdout.is_empty() {
        print!("{}", result.stdout);
    }
    if !result.stderr.is_empty() {
        eprint!("{}", result.stderr);
    }

    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("SQL fallo con exit code {}", result.exit_code),
        });
    }

    if dry_run {
        println!("\n[run-sql] dry-run completado — ROLLBACK ejecutado, nada se aplicó.");
    }

    Ok(())
}
