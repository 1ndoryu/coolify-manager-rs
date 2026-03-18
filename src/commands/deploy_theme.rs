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
        let theme_dir = format!("/var/www/html/wp-content/themes/{}", site.theme_name);
        let glory_dir = format!("{}/Glory", theme_dir);

        let previous_git = docker::docker_exec(
            &ssh,
            &wp_container,
            &format!("cd {} && git rev-parse HEAD", theme_dir),
        )
        .await
        .ok()
        .map(|result| result.stdout.trim().to_string())
        .filter(|hash| !hash.is_empty());

        let previous_glory_git = docker::docker_exec(
            &ssh,
            &wp_container,
            &format!("cd {} && git rev-parse HEAD 2>/dev/null", glory_dir),
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
            reportar_cambios_git(
                &ssh,
                &wp_container,
                &theme_dir,
                &glory_dir,
                previous_git.as_deref(),
                previous_glory_git.as_deref(),
            )
            .await;
            rollback_repositorios(
                &ssh,
                &wp_container,
                &theme_dir,
                &glory_dir,
                previous_git.as_deref(),
                previous_glory_git.as_deref(),
            )
            .await;
            return Err(error);
        }

        if let Err(error) = health_manager::assert_site_healthy(&settings, site, &ssh).await {
            reportar_cambios_git(
                &ssh,
                &wp_container,
                &theme_dir,
                &glory_dir,
                previous_git.as_deref(),
                previous_glory_git.as_deref(),
            )
            .await;
            rollback_repositorios(
                &ssh,
                &wp_container,
                &theme_dir,
                &glory_dir,
                previous_git.as_deref(),
                previous_glory_git.as_deref(),
            )
            .await;
            return Err(error);
        }

        /* QL11: Reportar cambios git despues de deploy exitoso */
        reportar_cambios_git(
            &ssh,
            &wp_container,
            &theme_dir,
            &glory_dir,
            previous_git.as_deref(),
            previous_glory_git.as_deref(),
        )
        .await;
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

async fn rollback_repositorios(
    ssh: &SshClient,
    container: &str,
    theme_dir: &str,
    glory_dir: &str,
    prev_theme: Option<&str>,
    prev_glory: Option<&str>,
) {
    if let Some(commit) = prev_theme {
        let rollback = format!("cd {} && git reset --hard {}", theme_dir, commit);
        match docker::docker_exec(ssh, container, &rollback).await {
            Ok(result) if result.success() => tracing::info!("Rollback del tema aplicado a {}", commit),
            Ok(result) => tracing::warn!("Rollback del tema fallo: {}", result.stderr),
            Err(error) => tracing::warn!("Rollback del tema no pudo ejecutarse: {error}"),
        }
    }

    if let Some(commit) = prev_glory {
        let rollback = format!("cd {} && git reset --hard {}", glory_dir, commit);
        match docker::docker_exec(ssh, container, &rollback).await {
            Ok(result) if result.success() => tracing::info!("Rollback de Glory aplicado a {}", commit),
            Ok(result) => tracing::warn!("Rollback de Glory fallo: {}", result.stderr),
            Err(error) => tracing::warn!("Rollback de Glory no pudo ejecutarse: {error}"),
        }
    }
}

/* QL11: Muestra resumen de cambios git al usuario despues de deploy.
 * Compara hashes antes/despues para tema y libreria Glory. */
async fn reportar_cambios_git(
    ssh: &SshClient,
    container: &str,
    theme_dir: &str,
    glory_dir: &str,
    prev_theme: Option<&str>,
    prev_glory: Option<&str>,
) {
    println!("\n--- Cambios Git ---");

    /* Tema principal */
    let new_theme = docker::docker_exec(ssh, container, &format!("cd {} && git rev-parse HEAD", theme_dir))
        .await
        .ok()
        .map(|r| r.stdout.trim().to_string())
        .filter(|h| !h.is_empty());

    match (prev_theme, new_theme.as_deref()) {
        (Some(antes), Some(despues)) if antes != despues => {
            println!("Tema: {} -> {}", &antes[..8.min(antes.len())], &despues[..8.min(despues.len())]);
            if let Ok(log) = docker::docker_exec(
                ssh,
                container,
                &format!("cd {} && git log --oneline --stat {}..{}", theme_dir, antes, despues),
            )
            .await
            {
                if !log.stdout.trim().is_empty() {
                    println!("{}", log.stdout.trim());
                }
            }
        }
        (Some(_), Some(_)) => println!("Tema: sin cambios"),
        _ => println!("Tema: hash anterior no disponible"),
    }

    /* Libreria Glory */
    let new_glory = docker::docker_exec(ssh, container, &format!("cd {} && git rev-parse HEAD 2>/dev/null", glory_dir))
        .await
        .ok()
        .map(|r| r.stdout.trim().to_string())
        .filter(|h| !h.is_empty());

    match (prev_glory, new_glory.as_deref()) {
        (Some(antes), Some(despues)) if antes != despues => {
            println!("Glory: {} -> {}", &antes[..8.min(antes.len())], &despues[..8.min(despues.len())]);
            if let Ok(log) = docker::docker_exec(
                ssh,
                container,
                &format!("cd {} && git log --oneline --stat {}..{}", glory_dir, antes, despues),
            )
            .await
            {
                if !log.stdout.trim().is_empty() {
                    println!("{}", log.stdout.trim());
                }
            }
        }
        (Some(_), Some(_)) => println!("Glory: sin cambios"),
        _ => println!("Glory: hash anterior no disponible"),
    }

    println!("-------------------");
}
