/*
 * Comando: deploy-theme
 * Despliega o actualiza el tema Glory en un sitio existente.
 * Deploy y backup son procesos independientes — aqui solo se despliega.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::{health_manager, theme_manager};
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
    let target = settings.resolve_site_target(site)?;

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let wp_container = docker::find_wordpress_container(&ssh, stack_uuid).await?;

    let effective_smtp: Option<SmtpConfig> = site
        .smtp_config
        .clone()
        .or_else(|| settings.smtp.as_ref().map(|s| s.as_smtp_config()));

    if update {
        let previous_git = docker::docker_exec(
            &ssh,
            &wp_container,
            &format!("cd /var/www/html/wp-content/themes/{} && git rev-parse HEAD", site.theme_name),
        )
        .await
        .ok()
        .map(|result| result.stdout.trim().to_string())
        .filter(|hash| !hash.is_empty());

        let update_result = theme_manager::update_glory_theme(
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
            site.disable_wp_cron,
        )
        .await;

        if let Err(error) = update_result {
            if let Some(ref commit) = previous_git {
                let rollback = format!(
                    "cd /var/www/html/wp-content/themes/{} && git reset --hard {}",
                    site.theme_name, commit
                );
                let _ = docker::docker_exec(&ssh, &wp_container, &rollback).await;
            }
            return Err(error);
        }

        if let Err(error) = health_manager::assert_site_healthy(&settings, site, &ssh).await {
            if let Some(ref commit) = previous_git {
                let rollback = format!(
                    "cd /var/www/html/wp-content/themes/{} && git reset --hard {}",
                    site.theme_name, commit
                );
                let _ = docker::docker_exec(&ssh, &wp_container, &rollback).await;
            }
            return Err(error);
        }
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
