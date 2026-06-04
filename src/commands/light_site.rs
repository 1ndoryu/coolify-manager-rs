use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::lightweight_runtime_manager::{self, LightweightSiteAction};

use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub async fn execute(
    config_path: &Path,
    target_name: &str,
    site_name: &str,
    action: &str,
    fqdn: Option<&str>,
    access_user: Option<&str>,
    access_password: Option<&str>,
    delete_volumes: bool,
    json: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let target = settings.get_target(target_name)?.clone();
    let action = LightweightSiteAction::parse(action)?;
    let report = lightweight_runtime_manager::control_lightweight_site(
        &target,
        site_name,
        action,
        fqdn,
        access_user,
        access_password,
        delete_volumes,
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
        "Target: {} ({}) | Site: {} | Action: {} | Status: {}",
        report.target, report.target_ip, report.site, report.action, report.status
    );
    if let Some(fqdn) = report.fqdn {
        println!("FQDN: {fqdn}");
    }
    for note in report.notes {
        println!("- {note}");
    }
    Ok(())
}
