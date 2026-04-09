use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::{alert_manager, health_manager};

use std::path::Path;

/* [N2] Health check con soporte para --all (todos los sitios) y --alert (enviar email).
 * --all itera todos los sitios configurados.
 * --alert envia email via SMTP cuando un sitio esta caido.
 * Sin --all, se verifica el sitio indicado por --name. */
pub async fn execute(
    config_path: &Path,
    site_name: Option<&str>,
    all: bool,
    alert: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;

    if all {
        let unhealthy = alert_manager::check_and_alert_all_sites(&settings, config_path).await?;
        if !unhealthy.is_empty() && !alert {
            return Err(CoolifyError::Validation(format!(
                "{} sitio(s) con problemas",
                unhealthy.len()
            )));
        }
        return Ok(());
    }

    let name = site_name.ok_or_else(|| {
        CoolifyError::Validation("Se requiere --name o --all".to_string())
    })?;
    let site = settings.get_site(name)?;
    validation::assert_site_ready(site)?;
    let target = settings.resolve_site_target(site)?;

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let report = health_manager::run_site_health_check(&settings, site, &ssh).await?;
    println!(
        "Health {} | http_ok={} app_ok={} fatal_logs={}",
        report.site_name, report.http_ok, report.app_ok, report.fatal_log_detected
    );
    for detail in &report.details {
        println!("- {detail}");
    }

    if !report.healthy() {
        if alert {
            if let Err(e) = alert_manager::alert_site_down(&settings, &report).await {
                tracing::error!("No se pudo enviar alerta: {e}");
            }
        }
        return Err(CoolifyError::Validation(format!(
            "Health check fallo para '{name}'"
        )));
    }

    Ok(())
}
