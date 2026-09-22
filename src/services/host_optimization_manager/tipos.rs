/* Split 119A-5 de host_optimization_manager.rs — tipos y constantes internas.
 * Re-exportados sin cambios desde host_optimization_manager/mod.rs. */

use serde::Serialize;

pub(super) const SYSCTL_CONFIG_PATH: &str = "/etc/sysctl.d/99-coolify-manager-host.conf";
pub(super) const SWAPFILE_PATH: &str = "/swapfile";
pub(super) const THP_SERVICE_PATH: &str = "/etc/systemd/system/cm-disable-thp.service";
pub(super) const DOCKER_DAEMON_CONFIG_PATH: &str = "/etc/docker/daemon.json";

/* [235A-1] Las optimizaciones host-level deben pasar por coolify-manager-rs para que
 * swap, sysctl y diagnostico queden repetibles y auditables sin depender de SSH manual. */
#[derive(Debug, Clone)]
pub struct HostOptimizationRequest {
    pub swap_gb: u16,
    pub swappiness: u8,
    pub vfs_cache_pressure: u16,
    pub overcommit_memory: u8,
    pub disable_thp: bool,
    pub docker_live_restore: bool,
    pub dry_run: bool,
    pub samples: u8,
    pub interval_seconds: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostOptimizationReport {
    pub target: String,
    pub os_name: String,
    pub load_average: String,
    pub pressure_summary: String,
    pub memory_summary: String,
    pub sampling_summary: String,
    pub ssh_sessions_summary: String,
    pub ssh_recent_summary: String,
    pub swap_before: String,
    pub swap_after: String,
    pub sysctl_summary: String,
    pub thp_summary: String,
    pub docker_runtime_summary: String,
    pub top_processes: String,
    pub docker_stats: String,
    pub applied_steps: Vec<String>,
    pub recommendations: Vec<String>,
}

pub(super) struct HostSnapshot {
    pub(super) os_name: String,
    pub(super) load_average: String,
    pub(super) pressure_summary: String,
    pub(super) memory_summary: String,
    pub(super) sampling_summary: String,
    pub(super) ssh_sessions_summary: String,
    pub(super) ssh_recent_summary: String,
    pub(super) swap_summary: String,
    pub(super) sysctl_summary: String,
    pub(super) thp_summary: String,
    pub(super) docker_runtime_summary: String,
    pub(super) top_processes: String,
    pub(super) docker_stats: String,
    pub(super) cpu_count: usize,
}
