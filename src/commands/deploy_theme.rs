/*
 * Comando: deploy-theme
 * Despliega o actualiza el tema Glory en un sitio existente.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::theme_manager;
use crate::domain::SmtpConfig;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    glory_branch: Option<&str>,
    library_branch: Option<&str>,
    update: bool,
    skip_react: bool,
    force: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    let glory_branch = glory_branch.unwrap_or(&site.glory_branch);
    let library_branch = library_branch.unwrap_or(&site.library_branch);
    let stack_uuid = site.stack_uuid.as_deref().unwrap();

    let mut ssh = SshClient::new(
        &settings.vps.ip,
        &settings.vps.user,
        settings.vps.ssh_key.as_deref(),
    );
    ssh.connect().await?;

    let wp_container = docker::find_wordpress_container(&ssh, stack_uuid).await?;

    /* Resolver SMTP: configuracion por sitio tiene prioridad, si no hay se usa la global */
    let effective_smtp: Option<SmtpConfig> = site
        .smtp_config
        .clone()
        .or_else(|| settings.smtp.as_ref().map(|s| s.as_smtp_config()));

    if update {
        theme_manager::update_glory_theme(
            &ssh,
            &wp_container,
            stack_uuid,
            &settings.glory,
            glory_branch,
            library_branch,
            &site.theme_name,
            skip_react,
            force,
            site.php_config.as_ref(),
            effective_smtp.as_ref(),
        )
        .await?;
    } else {
        theme_manager::install_glory_theme(
            &ssh,
            &wp_container,
            &settings.glory,
            glory_branch,
            library_branch,
            &site.theme_name,
            skip_react,
        )
        .await?;
    }

    println!("Tema desplegado exitosamente en '{site_name}'.");
    Ok(())
}
