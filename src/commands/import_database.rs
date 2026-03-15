/*
 * Comando: import-database
 * Importa un archivo SQL en la base de datos WordPress.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::{database_manager, site_manager};

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    sql_file: &Path,
    fix_urls: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;
    validation::validate_file_exists(sql_file)?;

    let stack_uuid = site.stack_uuid.as_deref().unwrap();
    let target = settings.resolve_site_target(site)?;

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    /* Subir archivo SQL al servidor y luego al contenedor MariaDB */
    let remote_tmp = format!("/tmp/{}_import.sql", site_name);
    ssh.upload_file(sql_file, &remote_tmp).await?;

    let wp_container = docker::find_wordpress_container(&ssh, stack_uuid).await?;
    let mariadb_container = docker::find_mariadb_container(&ssh, stack_uuid).await?;
    let (db_name, db_user, db_password) = database_manager::resolve_wordpress_credentials(&ssh, &wp_container).await?;

    database_manager::import_database(
        &ssh,
        &mariadb_container,
        sql_file,
        &db_name,
        &db_user,
        &db_password,
    )
    .await?;

    /* Fix URLs si se solicita */
    if fix_urls {
        site_manager::set_wordpress_urls(&ssh, &wp_container, &site.dominio).await?;
        println!("URLs corregidas a {}", site.dominio);
    }

    /* Limpiar archivo temporal */
    let _ = ssh.execute(&format!("rm -f {remote_tmp}")).await;

    println!("Base de datos importada exitosamente en '{site_name}'.");
    Ok(())
}
