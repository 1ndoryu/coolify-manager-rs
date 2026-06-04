use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::lightweight_runtime_manager;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    target_name: &str,
    site_name: &str,
    backup_id: &str,
    access_password: Option<&str>,
    skip_safety_snapshot: bool,
    json: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let target = settings.get_target(target_name)?.clone();
    let report = lightweight_runtime_manager::restore_lightweight_site_backup(
        &settings,
        config_path,
        &target,
        site_name,
        backup_id,
        access_password,
        skip_safety_snapshot,
    )
    .await?;

    if json {
        println!(
            "{}",
            serde_json::to_string(&report)
                .map_err(|error| CoolifyError::Validation(error.to_string()))?
        );
        return Ok(());
    }

    println!(
        "Target: {} ({}) | Site: {} | Backup: {} | Status: {}",
        report.target, report.target_ip, report.site, report.backup_id, report.status
    );
    if let Some(fqdn) = report.fqdn {
        println!("FQDN: {fqdn}");
    }
    if let Some(access_user) = report.access_user {
        println!("Access user: {access_user}");
    }
    if let Some(access_password) = report.access_password {
        println!("Access password: {access_password}");
    }
    for note in report.notes {
        println!("- {note}");
    }
    Ok(())
}
