use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::maintenance_window_manager::{
    self, MaintenanceWindowRequest,
};

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    target_name: Option<&str>,
    apply: bool,
    dry_run: bool,
    force_evaluate: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let request = MaintenanceWindowRequest {
        apply,
        dry_run,
        force_evaluate,
    };

    let report = match target_name {
        Some(name) => {
            let target = settings.get_target(name)?;
            maintenance_window_manager::evaluate_target(&settings, target, &request).await?
        }
        None => maintenance_window_manager::evaluate_default_vps(&settings, &request).await?,
    };

    println!("Target: {}", report.target);
    println!("Politica: {}", report.reboot_policy);
    println!("Decision: {}", report.decision);
    println!(
        "Bloqueado: {}",
        if report.blocked { "si" } else { "no" }
    );
    println!(
        "Reboot requerido: {}",
        if report.reboot_required { "si" } else { "no" }
    );
    println!(
        "Drift detectado: {}",
        if report.drift_detected { "si" } else { "no" }
    );
    println!("Kernel running: {}", report.running_kernel);
    println!("Kernel instalado: {}", report.installed_kernel);
    println!("Load: {}", report.load_average);
    println!("Pressure CPU: {}", report.cpu_pressure);
    println!("Pressure IO: {}", report.io_pressure);
    println!(
        "CPU control-plane: {:.2}%",
        report.control_plane_cpu_percent
    );
    println!("Ops activas: {}", report.critical_ops_summary);
    println!(
        "Mantenimiento aplicado: {}",
        if report.applied_maintenance { "si" } else { "no" }
    );
    println!(
        "Reboot programado: {}",
        if report.reboot_scheduled { "si" } else { "no" }
    );

    for site in report.sample_sites {
        println!(
            "- sample-site={} healthy={} details={}",
            site.site_name,
            if site.healthy { "si" } else { "no" },
            site.details.join(" | ")
        );
    }

    for note in report.notes {
        println!("- {note}");
    }

    Ok(())
}