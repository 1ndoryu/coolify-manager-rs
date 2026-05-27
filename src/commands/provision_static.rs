use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::lightweight_runtime_manager;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    target_name: &str,
    site_name: &str,
    fqdn: Option<&str>,
    access_user: Option<&str>,
    access_password: Option<&str>,
    json: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let target = settings.get_target(target_name)?.clone();
    let report = lightweight_runtime_manager::provision_static_site(
        &target,
        site_name,
        fqdn,
        access_user,
        access_password,
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
        "Target: {} ({}) | Site: {} | URL: {}",
        report.target, report.target_ip, report.deployment_id, report.public_url
    );
    println!("Root: {}", report.project_root);
    println!("SFTP: {}:{}", report.access_user, report.access_port);
    Ok(())
}