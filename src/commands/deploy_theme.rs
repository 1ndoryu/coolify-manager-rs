/*
 * [F8] Comando: deploy-theme
 * Despliega o actualiza el tema Glory en un sitio existente.
 * Pre-deploy: backup automatico del sitio (salvo --skip-backup).
 * Post-deploy: health check de TODOS los sitios del servidor.
 */

use crate::config::Settings;
use crate::domain::{BackupTier, SmtpConfig};
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::{backup_manager, health_manager, theme_manager};

use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub async fn execute(
    config_path: &Path,
    site_name: &str,
    glory_branch: Option<&str>,
    library_branch: Option<&str>,
    update: bool,
    skip_react: bool,
    force: bool,
    skip_backup: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    /* [104A-46] Rust template no usa el flujo WordPress (git pull dentro del contenedor).
     * [154A-7] Tanto --update como deploy completo usan deploy-service (zero-downtime):
     * build imagen nueva en paralelo mientras el contenedor viejo sigue sirviendo,
     * luego swap rápido (~2-5s). Antes, --update usaba redeploy (STOP+START = 3-10min downtime). */
    if site.template == crate::domain::StackTemplate::Rust {
        if update {
            println!(
                "Sitio '{site_name}' es template Rust — usando deploy-service zero-downtime (build paralelo + swap)..."
            );
        } else {
            println!(
                "Sitio '{site_name}' es template Rust — usando deploy-service (build completo)..."
            );
        }
        return crate::commands::deploy_service::execute(
            config_path,
            site_name,
            false,
            false,
            false,
            skip_backup,
        )
        .await;
    }

    /* [F2] Safety check: verificar que todos los sitios del servidor existen en Coolify */
    println!("Verificando estado de sitios en Coolify...");
    validation::pre_deploy_safety_check(&settings, site_name).await?;

    let glory_branch = glory_branch.unwrap_or(&site.glory_branch);
    let library_branch = library_branch.unwrap_or(&site.library_branch);
    let stack_uuid = site.stack_uuid.as_deref().unwrap();
    let target = settings.resolve_site_target(site)?;

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    /* [F8] Backup automatico pre-deploy — protege contra desastres.
     * Si falla el backup, el deploy se cancela (salvo --skip-backup). */
    if !skip_backup && site.backup_policy.enabled {
        println!("Creando backup pre-deploy de '{site_name}'...");
        match backup_manager::create_site_backup(
            &settings,
            config_path,
            site,
            &ssh,
            BackupTier::Manual,
            Some("pre-deploy"),
        )
        .await
        {
            Ok(manifest) => {
                println!(
                    "Backup pre-deploy creado: {} ({} artifacts)",
                    manifest.backup_id,
                    manifest.artifacts.len()
                );
            }
            Err(error) => {
                tracing::warn!("Backup pre-deploy fallo: {error}");
                println!("ADVERTENCIA: Backup pre-deploy fallo: {error}");
                println!("Usa --skip-backup para forzar deploy sin backup.");
                return Err(error);
            }
        }
    } else if !skip_backup {
        tracing::info!("Backups deshabilitados para '{site_name}', saltando backup pre-deploy");
    }

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

    /* [F7] Health check de TODOS los demas sitios del mismo servidor.
     * Previene el escenario donde deployar un sitio rompe otros silenciosamente. */
    println!("Verificando salud de los demas sitios del servidor...");
    let mut unhealthy_sites = Vec::new();
    for other_site in &settings.sitios {
        if other_site.nombre == site_name {
            continue;
        }
        /* Solo chequear sitios del mismo target/servidor */
        let other_target = match settings.resolve_site_target(other_site) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if other_target.vps.ip != target.vps.ip {
            continue;
        }
        match health_manager::run_site_health_check(&settings, other_site, &ssh).await {
            Ok(report) if report.healthy() => {
                println!("  {} — OK", other_site.nombre);
            }
            Ok(report) => {
                let issues = report.details.join(", ");
                println!("  {} — FALLO: {}", other_site.nombre, issues);
                unhealthy_sites.push(other_site.nombre.clone());
            }
            Err(e) => {
                println!("  {} — ERROR: {}", other_site.nombre, e);
                unhealthy_sites.push(other_site.nombre.clone());
            }
        }
    }
    if !unhealthy_sites.is_empty() {
        println!(
            "\nADVERTENCIA: {} sitio(s) no saludable(s) tras el deploy: {}",
            unhealthy_sites.len(),
            unhealthy_sites.join(", ")
        );
    }

    Ok(())
}

/* [F10] Rollback completo: revierte git Y reinstala dependencias + rebuild.
 * Sin reinstalar deps, el rollback deja el sitio con vendor/node_modules del
 * codigo nuevo pero fuentes del codigo viejo — peor que antes del deploy. */
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
            Ok(result) if result.success() => {
                tracing::info!("Rollback del tema aplicado a {}", commit)
            }
            Ok(result) => tracing::warn!("Rollback del tema fallo: {}", result.stderr),
            Err(error) => tracing::warn!("Rollback del tema no pudo ejecutarse: {error}"),
        }
    }

    if let Some(commit) = prev_glory {
        let rollback = format!("cd {} && git reset --hard {}", glory_dir, commit);
        match docker::docker_exec(ssh, container, &rollback).await {
            Ok(result) if result.success() => {
                tracing::info!("Rollback de Glory aplicado a {}", commit)
            }
            Ok(result) => tracing::warn!("Rollback de Glory fallo: {}", result.stderr),
            Err(error) => tracing::warn!("Rollback de Glory no pudo ejecutarse: {error}"),
        }
    }

    /* Reinstalar dependencias con el codigo revertido */
    tracing::info!("Reinstalando dependencias tras rollback...");
    let composer_cmd = format!(
        "cd {theme_dir} && composer install --no-dev --optimize-autoloader --no-interaction 2>&1"
    );
    if let Ok(r) = docker::docker_exec(ssh, container, &composer_cmd).await {
        if r.success() {
            tracing::info!("Composer reinstalado tras rollback");
        } else {
            tracing::warn!("Composer install fallo en rollback: {}", r.stderr);
        }
    }

    let npm_cmd = format!(
        "cd {theme_dir} && npm install --no-audit --no-fund 2>&1 && npm run build 2>&1"
    );
    if let Ok(r) = docker::docker_exec(ssh, container, &npm_cmd).await {
        if r.success() {
            tracing::info!("npm install + build completado tras rollback");
        } else {
            tracing::warn!("npm build fallo en rollback: {}", r.stderr);
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
    let new_theme = docker::docker_exec(
        ssh,
        container,
        &format!("cd {} && git rev-parse HEAD", theme_dir),
    )
    .await
    .ok()
    .map(|r| r.stdout.trim().to_string())
    .filter(|h| !h.is_empty());

    match (prev_theme, new_theme.as_deref()) {
        (Some(antes), Some(despues)) if antes != despues => {
            println!(
                "Tema: {} -> {}",
                &antes[..8.min(antes.len())],
                &despues[..8.min(despues.len())]
            );
            if let Ok(log) = docker::docker_exec(
                ssh,
                container,
                &format!(
                    "cd {} && git log --oneline --stat {}..{}",
                    theme_dir, antes, despues
                ),
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
    let new_glory = docker::docker_exec(
        ssh,
        container,
        &format!("cd {} && git rev-parse HEAD 2>/dev/null", glory_dir),
    )
    .await
    .ok()
    .map(|r| r.stdout.trim().to_string())
    .filter(|h| !h.is_empty());

    match (prev_glory, new_glory.as_deref()) {
        (Some(antes), Some(despues)) if antes != despues => {
            println!(
                "Glory: {} -> {}",
                &antes[..8.min(antes.len())],
                &despues[..8.min(despues.len())]
            );
            if let Ok(log) = docker::docker_exec(
                ssh,
                container,
                &format!(
                    "cd {} && git log --oneline --stat {}..{}",
                    glory_dir, antes, despues
                ),
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
