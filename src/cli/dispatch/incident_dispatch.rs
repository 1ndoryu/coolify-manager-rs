/*
 * [257B-1] Dispatch de comandos de investigación de incidentes.
 */

use super::Command;
use coolify_manager::commands::{container, db_stats, env_toggle, incident};
use coolify_manager::config::Settings;
use coolify_manager::error::CoolifyError;
use coolify_manager::infra::ssh_client::SshClient;

use std::path::Path;

pub async fn dispatch_incident_commands(
    command: Command,
    config_path: &Path,
) -> Result<(), CoolifyError> {
    match command {
        Command::IncidentInvestigate { name, json, save } => {
            let settings = Settings::load(config_path)?;
            let save_str = save.as_ref().map(|p| p.to_string_lossy().to_string());
            incident::incident_investigate(&settings, &name, save_str.as_deref(), json).await?;
        }
        Command::IncidentLogs {
            name,
            since,
            until,
            patterns,
            json,
        } => {
            let settings = Settings::load(config_path)?;
            let site = settings
                .sitios
                .iter()
                .find(|s| s.nombre == name)
                .ok_or_else(|| {
                    CoolifyError::Validation(format!("Sitio '{}' no encontrado", name))
                })?;
            let target_config = settings.resolve_site_target(site)?;
            let mut ssh = SshClient::from_vps(&target_config.vps);
            ssh.connect().await?;
            let container_id = container::resolve_app_container_id(&settings, &name, &ssh).await?;
            let custom = patterns.map(|p| p.split(',').map(|s| s.trim().to_string()).collect());
            incident::incident_logs(
                &settings,
                &ssh,
                &container_id,
                &since,
                until.as_deref(),
                custom,
                json,
            )
            .await?;
        }
        Command::ContainerEvents {
            name,
            since,
            until,
            json,
        } => {
            let settings = Settings::load(config_path)?;
            let site = settings
                .sitios
                .iter()
                .find(|s| s.nombre == name)
                .ok_or_else(|| {
                    CoolifyError::Validation(format!("Sitio '{}' no encontrado", name))
                })?;
            let target_config = settings.resolve_site_target(site)?;
            let mut ssh = SshClient::from_vps(&target_config.vps);
            ssh.connect().await?;
            let container_id = container::resolve_app_container_id(&settings, &name, &ssh).await?;
            container::container_events(
                &settings,
                &ssh,
                &container_id,
                &since,
                until.as_deref(),
                json,
            )
            .await?;
        }
        Command::ContainerInspect { name, json } => {
            let settings = Settings::load(config_path)?;
            let site = settings
                .sitios
                .iter()
                .find(|s| s.nombre == name)
                .ok_or_else(|| {
                    CoolifyError::Validation(format!("Sitio '{}' no encontrado", name))
                })?;
            let target_config = settings.resolve_site_target(site)?;
            let mut ssh = SshClient::from_vps(&target_config.vps);
            ssh.connect().await?;
            let container_id = container::resolve_app_container_id(&settings, &name, &ssh).await?;
            container::inspect_container(&settings, &name, &ssh, &container_id, json).await?;
        }
        Command::ContainerStats { name, json } => {
            let settings = Settings::load(config_path)?;
            let site = settings
                .sitios
                .iter()
                .find(|s| s.nombre == name)
                .ok_or_else(|| {
                    CoolifyError::Validation(format!("Sitio '{}' no encontrado", name))
                })?;
            let target_config = settings.resolve_site_target(site)?;
            let mut ssh = SshClient::from_vps(&target_config.vps);
            ssh.connect().await?;
            let container_id = container::resolve_app_container_id(&settings, &name, &ssh).await?;
            container::container_stats(&settings, &ssh, &container_id, json).await?;
        }
        Command::DbStats {
            name,
            threshold,
            json,
        } => {
            let settings = Settings::load(config_path)?;
            db_stats::execute(&settings, &name, threshold, json).await?;
        }
        Command::EnvToggle {
            name,
            key,
            value,
            restart,
            dry_run,
        } => {
            env_toggle::execute(config_path, &name, &key, &value, restart, dry_run).await?;
        }
        _ => unreachable!("dispatch_incident_commands called with non-incident command"),
    }
    Ok(())
}
