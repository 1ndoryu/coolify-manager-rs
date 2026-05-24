use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::maintenance_window_manager::{
    self, ScheduleMaintenanceRequest,
};

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    target_name: &str,
    dry_run: bool,
    remove: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let target = settings.get_target(target_name)?;
    let request = ScheduleMaintenanceRequest { dry_run, remove };
    let report = maintenance_window_manager::schedule_target(&settings, target, &request).await?;

    println!("Target: {}", report.target);
    println!("Script: {}", report.script_path);
    println!("Service: {}", report.service_path);
    println!("Timer: {}", report.timer_path);
    println!(
        "Accion: {}",
        if report.removed { "removido" } else { "instalado" }
    );
    println!("Proxima ejecucion: {}", report.next_trigger_summary);
    for note in report.notes {
        println!("- {note}");
    }

    Ok(())
}