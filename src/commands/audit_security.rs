use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::security_audit_manager;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    target_name: Option<&str>,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;

    let report = match target_name {
        Some(name) => {
            let target = settings.get_target(name)?;
            security_audit_manager::audit_target(target).await?
        }
        None => security_audit_manager::audit_default_vps(&settings).await?,
    };

    println!("Target: {}", report.target);
    println!("SSH config: {}", report.sshd_summary);
    println!("SSH activity: {}", report.ssh_activity_summary);
    println!("Firewall: {}", report.firewall_summary);
    println!("Fail2ban: {}", report.fail2ban_summary);
    println!("Docker ports: {}", report.docker_ports_summary);
    println!("Host users: {}", report.user_summary);
    for recommendation in report.recommendations {
        println!("- {recommendation}");
    }

    Ok(())
}
