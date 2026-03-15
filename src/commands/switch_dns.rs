use crate::config::Settings;
use crate::domain::SiteConfig;
use crate::error::CoolifyError;
use crate::services::dns_manager;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    target_name: Option<&str>,
    target_ip: Option<&str>,
    dry_run: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site: SiteConfig = settings.get_site(site_name)?.clone();
    let resolved_target_ip = match (target_ip, target_name) {
        (Some(ip), _) => ip.to_string(),
        (None, Some(name)) => settings.get_target(name)?.vps.ip.clone(),
        (None, None) => settings.resolve_site_target(&site)?.vps.ip,
    };

    let report = dns_manager::switch_site_dns(&settings, &site, &resolved_target_ip, dry_run).await?;
    println!("DNS {} -> {} ({})", report.zone, report.target_ip, if dry_run { "dry-run" } else { "aplicado" });
    for action in report.actions {
        println!("- {} {} -> {} ({})", action.record_type, action.record_name, action.value, action.action);
    }
    Ok(())
}