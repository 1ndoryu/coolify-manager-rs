use crate::config::Settings;
use crate::domain::BackupTier;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::backup_manager;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    tier: &str,
    label: Option<&str>,
    list: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;

    if list {
        let entries = backup_manager::list_site_backups(&settings, config_path, site_name).await?;
        if entries.is_empty() {
            println!("No hay backups para '{site_name}'.");
        } else {
            println!("{:<40} | {:<8} | {}", "ID", "Tier", "Drive File ID");
            println!("{}", "-".repeat(80));
            for entry in entries {
                println!(
                    "{:<40} | {:<8} | {}",
                    entry.backup_id,
                    entry.tier,
                    entry.file_id,
                );
            }
        }
        return Ok(());
    }

    validation::assert_site_ready(site)?;
    let backup_tier = parse_tier(tier)?;
    let target = settings.resolve_site_target(site)?;

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let manifest = backup_manager::create_site_backup(&settings, config_path, site, &ssh, backup_tier, label).await?;
    println!("Backup creado: {}", manifest.backup_id);
    for note in manifest.notes {
        println!("- {}", note);
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