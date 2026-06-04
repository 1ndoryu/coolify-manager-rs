use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::lightweight_runtime_manager;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    target_name: &str,
    json: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let target = settings.get_target(target_name)?.clone();
    let report = lightweight_runtime_manager::inventory_light_target(&target).await?;

    if json {
        println!(
            "{}",
            serde_json::to_string(&report)
                .map_err(|error| CoolifyError::Validation(error.to_string()))?
        );
        return Ok(());
    }

    println!("Target: {} ({})", report.target, report.target_ip);
    if report.sites.is_empty() {
        println!("No hay sitios lightweight detectados en /srv/hosting.");
        return Ok(());
    }

    println!("{:<24} {:<12} {:<30} ROOT", "SITE", "STATUS", "FQDN");
    println!("{}", "-".repeat(96));
    for site in report.sites {
        println!(
            "{:<24} {:<12} {:<30} {}",
            site.name,
            site.status,
            site.fqdn.unwrap_or_else(|| "-".to_string()),
            site.project_root
        );
    }

    Ok(())
}
