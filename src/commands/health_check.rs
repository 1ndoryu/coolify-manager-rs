use crate::config::Settings;
use crate::domain::StackTemplate;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::{alert_manager, health_manager, health_manager::HealthReport};

use std::path::Path;

/* [N2] Health check con soporte para --all (todos los sitios) y --alert (enviar email).
 * --all itera todos los sitios configurados.
 * --alert envia email via SMTP cuando un sitio esta caido.
 * --repair reusa deploy-service sin build para recuperar fallos de red Rust.
 * Sin --all, se verifica el sitio indicado por --name. */
pub async fn execute(
    config_path: &Path,
    site_name: Option<&str>,
    all: bool,
    alert: bool,
    repair: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;

    if all {
        if repair {
            return Err(CoolifyError::Validation(
                "--repair requiere --name; no se ejecuta sobre --all".to_string(),
            ));
        }
        let unhealthy = alert_manager::check_and_alert_all_sites(&settings, config_path).await?;
        if !unhealthy.is_empty() && !alert {
            return Err(CoolifyError::Validation(format!(
                "{} sitio(s) con problemas",
                unhealthy.len()
            )));
        }
        return Ok(());
    }

    let name = site_name
        .ok_or_else(|| CoolifyError::Validation("Se requiere --name o --all".to_string()))?;
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
        if repair && is_repairable_rust_network_fault(&site.template, &report) {
            println!(
                "Fallo recuperable detectado; ejecutando deploy-service --skip-build --skip-backup..."
            );
            super::deploy_service::execute(config_path, name, true, false, false, true).await?;
            return Ok(());
        }
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

fn is_repairable_rust_network_fault(template: &StackTemplate, report: &HealthReport) -> bool {
    template == &StackTemplate::Rust
        && !report.fatal_log_detected
        && report.details.iter().any(|detail| {
            detail.contains("Rust network probe fallo")
                || detail.contains("HTTP devolvio estado 503")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report_with(details: Vec<&str>, fatal_log_detected: bool) -> HealthReport {
        HealthReport {
            site_name: "studio".to_string(),
            url: "https://nakomi.studio/healthz".to_string(),
            http_ok: false,
            app_ok: false,
            fatal_log_detected,
            status_code: Some(503),
            details: details.into_iter().map(str::to_string).collect(),
        }
    }

    #[test]
    fn rust_network_fault_is_repairable() {
        let report = report_with(vec!["Rust network probe fallo: exit=1"], false);

        assert!(is_repairable_rust_network_fault(
            &StackTemplate::Rust,
            &report
        ));
    }

    #[test]
    fn fatal_logs_are_not_repaired_automatically() {
        let report = report_with(vec!["HTTP devolvio estado 503"], true);

        assert!(!is_repairable_rust_network_fault(
            &StackTemplate::Rust,
            &report
        ));
    }

    #[test]
    fn wordpress_fault_is_not_repaired_as_rust_network() {
        let report = report_with(vec!["Rust network probe fallo: exit=1"], false);

        assert!(!is_repairable_rust_network_fault(
            &StackTemplate::Wordpress,
            &report
        ));
    }
}
