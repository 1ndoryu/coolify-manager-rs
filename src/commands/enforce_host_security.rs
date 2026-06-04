use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::host_security_manager::{self, EnforceHostSecurityRequest};

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    target_name: &str,
    dry_run: bool,
    apply: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let target = settings.get_target(target_name)?;
    let request = EnforceHostSecurityRequest { dry_run, apply };
    let report = host_security_manager::enforce_target(target, &request).await?;

    println!("Target: {}", report.target);
    println!("Modo: {}", if report.applied { "apply" } else { "preview" });
    println!("IP cliente actual: {}", report.current_client_ip);
    println!(
        "Validacion reconexion: {}",
        if report.reconnect_validated {
            "ok"
        } else {
            "no"
        }
    );
    println!("Respaldo UFW: {}", report.ufw_backup_path);
    println!("Jail fail2ban: {}", report.fail2ban_jail_path);
    println!("Firewall: {}", report.firewall_summary);
    println!("Fail2ban: {}", report.fail2ban_summary);
    for step in report.applied_steps {
        println!("- {step}");
    }
    for warning in report.warnings {
        println!("- {warning}");
    }

    Ok(())
}
