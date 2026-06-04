use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::host_maintenance_manager::{self, HostMaintenanceRequest};

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    target_name: Option<&str>,
    reboot: bool,
    dry_run: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let request = HostMaintenanceRequest { reboot, dry_run };

    let report = match target_name {
        Some(name) => {
            let target = settings.get_target(name)?;
            host_maintenance_manager::maintain_target(target, &request).await?
        }
        None => host_maintenance_manager::maintain_default_vps(&settings, &request).await?,
    };

    println!("Target: {}", report.target);
    println!("SO: {}", report.os_name);
    println!("Estado paquetes: {}", report.package_summary);
    println!("Kernel running: {}", report.running_kernel);
    println!("Kernel instalado: {}", report.installed_kernel);
    println!(
        "Reinicio requerido: {}",
        if report.reboot_required { "si" } else { "no" }
    );
    println!(
        "Reinicio programado: {}",
        if report.reboot_scheduled { "si" } else { "no" }
    );
    for step in report.applied_steps {
        println!("- {step}");
    }
    for recommendation in report.recommendations {
        println!("- {recommendation}");
    }

    Ok(())
}
