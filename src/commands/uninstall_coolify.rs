use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::target_bootstrap_manager;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    target_name: &str,
    purge_data: bool,
    dry_run: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let target = settings.get_target(target_name)?.clone();
    let report = target_bootstrap_manager::uninstall_coolify(&target, purge_data, dry_run).await?;

    println!(
        "Coolify desinstalado en '{}' (modo={})",
        report.target,
        if report.dry_run { "dry-run" } else { "apply" }
    );
    for note in report.notes {
        println!("- {}", note);
    }
    Ok(())
}
