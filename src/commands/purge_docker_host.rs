use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::docker_host_cleanup_manager;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    target_name: &str,
    all_data: bool,
    dry_run: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let target = settings.get_target(target_name)?.clone();
    let report = docker_host_cleanup_manager::purge_target(&target, all_data, dry_run).await?;

    println!(
        "Docker purgado en '{}' (modo={}, all_data={})",
        report.target,
        if report.dry_run { "dry-run" } else { "apply" },
        if report.all_data { "true" } else { "false" }
    );
    for note in report.notes {
        println!("- {}", note);
    }
    Ok(())
}