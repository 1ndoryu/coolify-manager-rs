/*
 * API publica de coolify-manager.
 * Capa de abstraccion entre los servicios internos y los consumidores (CLI, MCP, GUI/Tauri).
 * Retorna datos estructurados serializables — no imprime a stdout.
 */

mod site_commands;
pub mod types;

pub use site_commands::create_site;

use crate::commands;
use crate::config::Settings;
use crate::domain::{BackupTier, SiteConfig};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::infra::{docker, validation};
use crate::services::{audit_manager, backup_manager, health_manager};
use chrono::Utc;
use std::collections::{BTreeMap, HashMap};
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

pub async fn list_targets(config_path: &Path) -> Result<TargetsResponse, CoolifyError> {
    let settings = Settings::load(config_path)?;
    let mut targets = Vec::new();
    let mut all_targets = Vec::with_capacity(settings.targets.len() + 1);
    all_targets.push(settings.default_target());
    all_targets.extend(settings.targets.iter().cloned());

    for target in all_targets {
        let site_count = settings
            .sitios
            .iter()
            .filter(|site| site.target.as_deref().unwrap_or("default") == target.name)
            .count();

        targets.push(TargetSummary {
            name: target.name,
            host: target.vps.ip,
            user: target.vps.user,
            coolify_url: target.coolify.base_url,
            site_count,
        });
    }

    Ok(TargetsResponse {
        default_target: "default".to_string(),
        config_path: config_path.display().to_string(),
        targets,
    })
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

pub async fn list_all_backups(config_path: &Path) -> Result<BackupsOverviewResponse, CoolifyError> {
    let settings = Settings::load(config_path)?;
    let mut backups = Vec::new();
    let report =
        backup_manager::list_all_site_backups(&settings, config_path, &settings.sitios).await?;

    for site_report in report.sites {
        let site = settings.get_site(&site_report.site_name)?;
        for entry in site_report.entries {
            backups.push(BackupOverviewSummary {
                site_name: site.nombre.clone(),
                domain: site.dominio.clone(),
                target: site.target.as_deref().unwrap_or("default").to_string(),
                template: format!("{:?}", site.template),
                backup_id: entry.backup_id.clone(),
                tier: entry.tier.to_string(),
                status: "Ready".to_string(),
                created_at: entry.backup_id,
                label: None,
                artifact_count: 1,
            });
        }
    }

    backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(BackupsOverviewResponse {
        backups,
        errors: report
            .errors
            .into_iter()
            .map(|error| BackupListError {
                site_name: error.site_name,
                message: error.message,
            })
            .collect(),
    })
}

pub async fn audit_vps(
    config_path: &Path,
    target_name: Option<&str>,
) -> Result<AuditResponse, CoolifyError> {
    let settings = Settings::load(config_path)?;
    let normalized_target = target_name
        .map(str::trim)
        .filter(|name| !name.is_empty() && *name != "default");

    let report = match normalized_target {
        Some(name) => {
            let target = settings.get_target(name)?;
            audit_manager::audit_target(target).await?
        }
        None => audit_manager::audit_default_vps(&settings).await?,
    };

    Ok(AuditResponse {
        target: report.target,
        load_1m: parse_load_average(&report.load_average, 0),
        load_5m: parse_load_average(&report.load_average, 1),
        load_15m: parse_load_average(&report.load_average, 2),
        memory_used_mb: parse_named_mb(&report.memory_summary, "used"),
        memory_free_mb: parse_named_mb(&report.memory_summary, "free"),
        memory_total_mb: parse_named_mb(&report.memory_summary, "total"),
        disk_use_percent: parse_disk_use_percent(&report.disk_summary),
        load_average: report.load_average,
        memory_summary: report.memory_summary,
        disk_summary: report.disk_summary,
        docker_summary: report.docker_summary,
        security_summary: report.security_summary,
        recommendations: report.recommendations,
    })
}

pub async fn deployment_metrics(
    config_path: &Path,
) -> Result<DeploymentMetricsResponse, CoolifyError> {
    /* [105A-22] La GUI muestra CPU/RAM por despliegue con datos reales de Docker.
     * Gotcha: se agrupa por target para evitar abrir una conexion SSH por sitio. */
    let settings = Settings::load(config_path)?;
    let generated_at = Utc::now().to_rfc3339();
    let mut grouped: BTreeMap<
        String,
        (crate::config::DeploymentTargetConfig, Vec<MetricQuerySite>),
    > = BTreeMap::new();

    for site in settings
        .sitios
        .iter()
        .filter(|site| site.stack_uuid.is_some())
    {
        let target = settings.resolve_site_target(site)?;
        let query_site = MetricQuerySite {
            name: site.nombre.clone(),
            stack_uuid: site.stack_uuid.clone().unwrap_or_default(),
        };

        grouped
            .entry(target.name.clone())
            .or_insert_with(|| (target, Vec::new()))
            .1
            .push(query_site);
    }

    let mut metrics = Vec::new();
    for (target_name, (target, sites)) in grouped {
        let mut ssh = SshClient::from_vps(&target.vps);
        ssh.connect().await?;
        let output = ssh.execute(&build_metrics_script(&sites)).await?;
        metrics.extend(parse_metrics_output(
            &target_name,
            &generated_at,
            &sites,
            &output.stdout,
        ));
    }

    Ok(DeploymentMetricsResponse {
        generated_at,
        metrics,
    })
}

pub async fn view_logs(
    config_path: &Path,
    site_name: &str,
    lines: u32,
    container_target: Option<&str>,
) -> Result<LogsResponse, CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;
    let stack_uuid = site.stack_uuid.as_deref().unwrap_or_default();
    let target = settings.resolve_site_target(site)?;
    let selected_target = container_target.unwrap_or_else(|| default_container_target(site));
    let bounded_lines = lines.clamp(20, 500);

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let container_id = find_log_container(&ssh, stack_uuid, selected_target).await?;
    let output = ssh
        .execute(&format!(
            "docker logs --tail {} {} 2>&1",
            bounded_lines,
            shell_quote(&container_id)
        ))
        .await?;

    Ok(LogsResponse {
        site_name: site_name.to_string(),
        container_target: selected_target.to_string(),
        lines: bounded_lines,
        content: output.stdout,
        stderr: output.stderr,
        exit_code: output.exit_code,
    })
}

pub async fn manual_backup(
    config_path: &Path,
    site_name: &str,
) -> Result<OperationResult, CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;
    let target = settings.resolve_site_target(site)?;
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;
    let manifest = backup_manager::create_site_backup(
        &settings,
        config_path,
        site,
        &ssh,
        BackupTier::Manual,
        Some("gui-manual"),
    )
    .await?;

    Ok(OperationResult {
        success: true,
        message: format!(
            "Copia manual creada para '{}': {}",
            site_name, manifest.backup_id
        ),
        details: Some(manifest.notes.join("\n")),
    })
}

pub async fn restart_site(
    config_path: &Path,
    site_name: &str,
) -> Result<OperationResult, CoolifyError> {
    /* [105A-17] Las acciones de la GUI reutilizan comandos existentes para conservar guardas.
     * Gotcha: restart_site aplica correcciones de bind mount para Rust; no llamar la API cruda aqui. */
    commands::restart_site::execute(config_path, Some(site_name), false, false, false).await?;

    Ok(OperationResult {
        success: true,
        message: format!("Reinicio solicitado para '{}'", site_name),
        details: Some("Coolify recibió la orden de restart. Ejecuta health si necesitas confirmar el estado final.".to_string()),
    })
}

pub async fn redeploy_site(
    config_path: &Path,
    site_name: &str,
) -> Result<OperationResult, CoolifyError> {
    commands::redeploy::execute(config_path, site_name, false).await?;
    Ok(OperationResult {
        success: true,
        message: format!("Redespliegue protegido completado para '{}'", site_name),
        details: Some(
            "Incluyó las guardas existentes de coolify-manager-rs para backup, compose y health."
                .to_string(),
        ),
    })
}

/* Envoltura de sync-env para consumidores de la capa API publica (MCP, GUI).
 * La implementacion real vive en commands::sync_env. */
pub async fn sync_env(
    config_path: &Path,
    site_name: &str,
    direction: &str,
    dry_run: bool,
    env_file: Option<&Path>,
) -> Result<OperationResult, CoolifyError> {
    commands::sync_env::execute(config_path, site_name, direction, dry_run, env_file, &[]).await?;
    Ok(OperationResult {
        success: true,
        message: format!("sync-env '{direction}' completado para '{site_name}'"),
        details: None,
    })
}

#[derive(Debug, Clone)]
struct MetricQuerySite {
    name: String,
    stack_uuid: String,
}

async fn find_log_container(
    ssh: &SshClient,
    stack_uuid: &str,
    container_target: &str,
) -> Result<String, CoolifyError> {
    match container_target {
        "mariadb" => docker::find_mariadb_container(ssh, stack_uuid).await,
        "postgres" => docker::find_postgres_container(ssh, stack_uuid).await,
        "websocket" => docker::find_websocket_container(ssh, stack_uuid).await,
        "app" => docker::find_app_container(ssh, stack_uuid).await,
        _ => docker::find_wordpress_container(ssh, stack_uuid).await,
    }
}

fn default_container_target(site: &SiteConfig) -> &'static str {
    if matches!(site.template, crate::domain::StackTemplate::Rust) {
        "app"
    } else {
        "wordpress"
    }
}

fn build_metrics_script(sites: &[MetricQuerySite]) -> String {
    let mut script = String::from("set -o pipefail; ");
    for site in sites {
        script.push_str(&format!(
            "site={}; uuid={}; ids=$(docker ps --filter \"name=$uuid\" -q | tr '\\n' ' '); if [ -z \"$ids\" ]; then printf '%s\\t\\t0%%\\t0B / 0B\\t0%%\\tstopped\\n' \"$site\"; else docker stats --no-stream --format \"$site\\t{{{{.Name}}}}\\t{{{{.CPUPerc}}}}\\t{{{{.MemUsage}}}}\\t{{{{.MemPerc}}}}\\trunning\" $ids; fi; ",
            shell_quote(&site.name),
            shell_quote(&site.stack_uuid)
        ));
    }

    format!("bash -lc {}", shell_quote(&script))
}

fn parse_metrics_output(
    target: &str,
    generated_at: &str,
    sites: &[MetricQuerySite],
    output: &str,
) -> Vec<DeploymentMetric> {
    let mut by_site: HashMap<String, DeploymentMetric> = sites
        .iter()
        .map(|site| {
            (
                site.name.clone(),
                DeploymentMetric {
                    site_name: site.name.clone(),
                    target: target.to_string(),
                    status: "sin-contenedores".to_string(),
                    total_cpu_percent: 0.0,
                    memory_used_bytes: 0,
                    memory_limit_bytes: 0,
                    memory_percent: 0.0,
                    containers: Vec::new(),
                    updated_at: generated_at.to_string(),
                },
            )
        })
        .collect();

    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let columns: Vec<&str> = line.split('\t').collect();
        if columns.len() < 6 {
            continue;
        }

        let site_name = columns[0].to_string();
        let container_name = columns[1].to_string();
        let status = columns[5].to_string();
        let entry = by_site
            .entry(site_name.clone())
            .or_insert(DeploymentMetric {
                site_name: site_name.clone(),
                target: target.to_string(),
                status: status.clone(),
                total_cpu_percent: 0.0,
                memory_used_bytes: 0,
                memory_limit_bytes: 0,
                memory_percent: 0.0,
                containers: Vec::new(),
                updated_at: generated_at.to_string(),
            });

        entry.status = status.clone();
        if status != "running" || container_name.is_empty() {
            continue;
        }

        let cpu_percent = parse_percent(columns[2]);
        let (memory_used_bytes, memory_limit_bytes) = parse_memory_usage(columns[3]);
        let memory_percent = parse_percent(columns[4]);
        entry.total_cpu_percent += cpu_percent;
        entry.memory_used_bytes += memory_used_bytes;
        entry.memory_limit_bytes += memory_limit_bytes;
        entry.containers.push(ContainerMetric {
            name: container_name,
            cpu_percent,
            memory_usage: columns[3].to_string(),
            memory_percent,
            memory_used_bytes,
            memory_limit_bytes,
        });
    }

    let mut metrics: Vec<DeploymentMetric> = by_site
        .into_values()
        .map(|mut metric| {
            if metric.memory_limit_bytes > 0 {
                metric.memory_percent =
                    (metric.memory_used_bytes as f32 / metric.memory_limit_bytes as f32) * 100.0;
            }
            metric
        })
        .collect();
    metrics.sort_by(|a, b| a.site_name.cmp(&b.site_name));
    metrics
}

fn parse_load_average(value: &str, index: usize) -> Option<f32> {
    value.split_whitespace().nth(index)?.parse::<f32>().ok()
}

fn parse_named_mb(value: &str, key: &str) -> Option<u64> {
    let prefix = format!("{key}=");
    value
        .split_whitespace()
        .find_map(|part| part.strip_prefix(&prefix))?
        .trim_end_matches("MB")
        .parse::<u64>()
        .ok()
}

fn parse_disk_use_percent(value: &str) -> Option<f32> {
    value
        .split_whitespace()
        .find_map(|part| part.strip_prefix("use="))?
        .trim_end_matches('%')
        .parse::<f32>()
        .ok()
}

fn parse_percent(value: &str) -> f32 {
    value
        .trim()
        .trim_end_matches('%')
        .parse::<f32>()
        .unwrap_or(0.0)
}

fn parse_memory_usage(value: &str) -> (u64, u64) {
    let mut parts = value.split('/').map(str::trim);
    let used = parts.next().map(parse_memory_value).unwrap_or(0);
    let limit = parts.next().map(parse_memory_value).unwrap_or(0);
    (used, limit)
}

fn parse_memory_value(value: &str) -> u64 {
    let trimmed = value.trim();
    let number: String = trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect();
    let unit = trimmed[number.len()..].trim().to_ascii_lowercase();
    let base = number.parse::<f64>().unwrap_or(0.0);
    let multiplier = match unit.as_str() {
        "gib" | "gb" | "g" => 1024_f64.powi(3),
        "mib" | "mb" | "m" => 1024_f64.powi(2),
        "kib" | "kb" | "k" => 1024_f64,
        _ => 1.0,
    };
    (base * multiplier) as u64
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
