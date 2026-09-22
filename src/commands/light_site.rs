use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::lightweight_runtime_manager::{self, LightweightSiteAction};

use std::path::Path;

/* Params del comando light-site (119A-6). Todo Copy: `= *p` sin mover. */
#[derive(Clone, Copy)]
pub struct ParamsLightSite<'a> {
    pub config_path: &'a Path,
    pub target_name: &'a str,
    pub site_name: &'a str,
    pub action: &'a str,
    pub fqdn: Option<&'a str>,
    pub access_user: Option<&'a str>,
    pub access_password: Option<&'a str>,
    pub delete_volumes: bool,
    pub json: bool,
}

pub async fn execute(p: &ParamsLightSite<'_>) -> std::result::Result<(), CoolifyError> {
    let ParamsLightSite {
        config_path,
        target_name,
        site_name,
        action,
        fqdn,
        access_user,
        access_password,
        delete_volumes,
        json,
    } = *p;
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
