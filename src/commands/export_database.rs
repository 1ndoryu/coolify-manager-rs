/*
 * Comando: export-database
 * Exporta la base de datos WordPress a un archivo SQL local.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::database_manager;

use chrono::Local;
use std::path::{Path, PathBuf};

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    output_path: Option<&Path>,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    let stack_uuid = site.stack_uuid.as_deref().unwrap();
    let target = settings.resolve_site_target(site)?;

    /* Generar ruta de salida con timestamp si no se especifica */
    let output = match output_path {
        Some(p) => p.to_path_buf(),
        None => {
            let timestamp = Local::now().format("%Y%m%d_%H%M%S");
            PathBuf::from(format!("{site_name}_{timestamp}.sql"))
        }
    };

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let wp_container = docker::find_wordpress_container(&ssh, stack_uuid).await?;
    let mariadb_container = docker::find_mariadb_container(&ssh, stack_uuid).await?;
    let (db_name, db_user, db_password) = database_manager::resolve_wordpress_credentials(&ssh, &wp_container).await?;

    database_manager::export_database(
        &ssh,
        &mariadb_container,
        &db_name,
        &db_user,
        &db_password,
        &output,
    )
    .await?;

    println!("Base de datos exportada a: {}", output.display());
    Ok(())
}
