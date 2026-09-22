/* Split 119A-5 de maintenance_window_manager.rs — tipos de request/reporte.
 * Re-exportados sin cambios desde maintenance_window_manager/mod.rs. */

use serde::Serialize;

#[derive(Debug, Clone)]
pub struct MaintenanceWindowRequest {
    pub apply: bool,
    pub dry_run: bool,
    pub force_evaluate: bool,
}

#[derive(Debug, Clone)]
pub struct ScheduleMaintenanceRequest {
    pub dry_run: bool,
    pub remove: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaintenanceSiteHealth {
    pub site_name: String,
    pub healthy: bool,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaintenanceWindowReport {
    pub target: String,
    pub reboot_policy: String,
    pub decision: String,
    pub blocked: bool,
    pub reboot_required: bool,
    pub drift_detected: bool,
    pub running_kernel: String,
    pub installed_kernel: String,
    pub load_average: String,
    pub cpu_pressure: String,
    pub io_pressure: String,
    pub control_plane_cpu_percent: f32,
    pub critical_ops_summary: String,
    pub applied_maintenance: bool,
    pub reboot_scheduled: bool,
    pub sample_sites: Vec<MaintenanceSiteHealth>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScheduleMaintenanceReport {
    pub target: String,
    pub script_path: String,
    pub service_path: String,
    pub timer_path: String,
    pub removed: bool,
    pub next_trigger_summary: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct DriftSnapshot {
    pub(super) load15: f32,
    pub(super) cpu_count: u32,
    pub(super) cpu_psi_some_avg10: f32,
    pub(super) io_psi_full_avg10: f32,
    pub(super) control_plane_cpu_percent: f32,
}
