use crate::config::Settings;
use crate::domain::BackupTier;
use crate::error::CoolifyError;
use crate::services::lightweight_runtime_manager;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    target_name: &str,
    site_name: &str,
    tier: &str,
    label: Option<&str>,
    list: bool,
    json: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let target = settings.get_target(target_name)?.clone();

    if list {
        let report = lightweight_runtime_manager::list_lightweight_site_backups(
            &settings,
            config_path,
            &target,
            site_name,
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
            "Target: {} ({}) | Site: {}",
            report.target, report.target_ip, report.site
        );
        if report.entries.is_empty() {
            println!("No hay backups remotos para este sitio lightweight.");
            return Ok(());
        }

        println!("{:<24} {:<8} {:<36} FILE", "ID", "TIER", "REMOTE ID");
        println!("{}", "-".repeat(96));
        for entry in report.entries {
            println!(
                "{:<24} {:<8} {:<36} {}",
                entry.backup_id, entry.tier, entry.file_id, entry.file_name
            );
        }
        return Ok(());
    }

    let report = lightweight_runtime_manager::create_lightweight_site_backup(
        &settings,
        config_path,
        &target,
        site_name,
        parse_tier(tier)?,
        label,
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
        "Target: {} ({}) | Site: {} | Backup: {} | Tier: {}",
        report.target, report.target_ip, report.site, report.backup_id, report.tier
    );
    for note in report.notes {
        println!("- {note}");
    }
    Ok(())
}

fn parse_tier(value: &str) -> std::result::Result<BackupTier, CoolifyError> {
    match value {
        "daily" => Ok(BackupTier::Daily),
        "weekly" => Ok(BackupTier::Weekly),
        "manual" => Ok(BackupTier::Manual),
        _ => Err(CoolifyError::Validation(format!("Tier invalido: {value}"))),
    }
}
