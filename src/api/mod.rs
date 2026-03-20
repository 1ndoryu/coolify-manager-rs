/*
 * API publica de coolify-manager.
 * Capa de abstraccion entre los servicios internos y los consumidores (CLI, MCP, GUI/Tauri).
 * Retorna datos estructurados serializables — no imprime a stdout.
 */

pub mod types;

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::{audit_manager, backup_manager, health_manager};
use std::path::Path;
use types::*;

pub async fn list_sites(config_path: &Path) -> Result<SitesResponse, CoolifyError> {
    let settings = Settings::load(config_path)?;
    let mut sites: Vec<SiteSummary> = Vec::new();

    for site in &settings.sitios {
        let target = site.target.as_deref().unwrap_or("default");
        sites.push(SiteSummary {
            name: site.nombre.clone(),
            domain: site.dominio.clone(),
            target: target.to_string(),
            stack_uuid: site.stack_uuid.clone().unwrap_or_default(),
            template: format!("{:?}", site.template),
        });
    }

    let minecraft: Vec<MinecraftSummary> = settings
        .minecraft
        .iter()
        .map(|mc| MinecraftSummary {
            name: mc.server_name.clone(),
            memory: mc.memory.clone(),
            max_players: mc.max_players,
        })
        .collect();

    Ok(SitesResponse { sites, minecraft })
}

pub async fn health_check(
    config_path: &Path,
    site_name: &str,
) -> Result<HealthResponse, CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;
    let target = settings.resolve_site_target(site)?;

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let report = health_manager::run_site_health_check(&settings, site, &ssh).await?;
    let is_healthy = report.healthy();

    Ok(HealthResponse {
        site_name: report.site_name,
        url: report.url,
        http_ok: report.http_ok,
        app_ok: report.app_ok,
        fatal_log_detected: report.fatal_log_detected,
        status_code: report.status_code,
        healthy: is_healthy,
        details: report.details,
    })
}

pub async fn list_backups(
    config_path: &Path,
    site_name: &str,
) -> Result<BackupsResponse, CoolifyError> {
    let settings = Settings::load(config_path)?;
    let _site = settings.get_site(site_name)?;
    let entries = backup_manager::list_site_backups(&settings, config_path, site_name).await?;

    let backups: Vec<BackupSummary> = entries
        .iter()
        .map(|e| BackupSummary {
            backup_id: e.backup_id.clone(),
            tier: e.tier.to_string(),
            status: "Ready".to_string(),
            created_at: e.backup_id.clone(),
            label: None,
            artifact_count: 1,
        })
        .collect();

    Ok(BackupsResponse {
        site_name: site_name.to_string(),
        backups,
    })
}

pub async fn audit_vps(
    config_path: &Path,
    target_name: Option<&str>,
) -> Result<AuditResponse, CoolifyError> {
    let settings = Settings::load(config_path)?;

    let report = match target_name {
        Some(name) => {
            let target = settings.get_target(name)?;
            audit_manager::audit_target(target).await?
        }
        None => audit_manager::audit_default_vps(&settings).await?,
    };

    Ok(AuditResponse {
        target: report.target,
        load_average: report.load_average,
        memory_summary: report.memory_summary,
        disk_summary: report.disk_summary,
        docker_summary: report.docker_summary,
        security_summary: report.security_summary,
        recommendations: report.recommendations,
    })
}
