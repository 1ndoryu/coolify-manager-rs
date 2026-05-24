use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::ssh_hardening_manager::{self, HardenSshRequest};

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    target_name: &str,
    dry_run: bool,
    apply: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let target = settings.get_target(target_name)?;
    let request = HardenSshRequest { dry_run, apply };
    let report = ssh_hardening_manager::harden_target(target, &request).await?;

    println!("Target: {}", report.target);
    println!("Modo: {}", if report.applied { "apply" } else { "preview" });
    println!("Archivo override: {}", report.override_path);
    println!("Respaldo: {}", report.backup_path);
    println!(
        "Validacion reconexion: {}",
        if report.reconnect_validated { "ok" } else { "no" }
    );
    for step in report.applied_steps {
        println!("- {step}");
    }
    for warning in report.warnings {
        println!("- {warning}");
    }

    Ok(())
}