use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::audit_manager;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    target_name: Option<&str>,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let report = match target_name {
        Some(name) => {
            let target = settings.get_target(name)?;
            audit_manager::audit_target(target).await?
        }
        None => audit_manager::audit_default_vps(&settings).await?,
    };

    println!("Target: {}", report.target);
    println!("Load: {}", report.load_average);
    println!("Memoria: {}", report.memory_summary);
    println!("Disco: {}", report.disk_summary);
    println!("Docker: {}", report.docker_summary);
    println!("Seguridad: {}", report.security_summary);
    for recommendation in report.recommendations {
        println!("- {recommendation}");
    }
    Ok(())
}
