use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::health_manager;

use std::path::Path;

pub async fn execute(config_path: &Path, site_name: &str) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;
    let target = settings.resolve_site_target(site)?;

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let report = health_manager::run_site_health_check(&settings, site, &ssh).await?;
    println!("Health {} | http_ok={} app_ok={} fatal_logs={}", report.site_name, report.http_ok, report.app_ok, report.fatal_log_detected);
    for detail in &report.details {
        println!("- {detail}");
    }

    if !report.healthy() {
        return Err(CoolifyError::Validation(format!("Health check fallo para '{site_name}'")));
    }

    Ok(())
}