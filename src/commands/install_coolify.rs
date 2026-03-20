use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::target_bootstrap_manager;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    target_name: &str,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let target = settings.get_target(target_name)?.clone();
    let report = target_bootstrap_manager::install_coolify(&target).await?;

    println!(
        "Coolify preparado en '{}' -> {}",
        report.target, report.access_url
    );
    for note in report.notes {
        println!("- {}", note);
    }
    Ok(())
}
