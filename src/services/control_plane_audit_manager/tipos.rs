/* Tipos del auditor del plano de control (119A-5 split control_plane_audit_manager). */

use serde::Serialize;

pub(super) const CONTROL_PLANE_CONTAINERS: &[&str] = &[
    "coolify",
    "coolify-db",
    "coolify-redis",
    "coolify-realtime",
    "coolify-sentinel",
];
pub(super) const COOLIFY_PROXY_CONTAINER: &str = "coolify-proxy";

#[derive(Debug, Clone, Serialize)]
pub struct ControlPlaneAuditReport {
    pub target: String,
    pub load_average: String,
    pub dominance_summary: String,
    pub container_summary: String,
    pub coolify_process_summary: String,
    pub supervisor_summary: String,
    pub scheduler_summary: String,
    pub horizon_summary: String,
    pub failed_job_summary: String,
    pub redis_summary: String,
    pub queue_summary: String,
    pub logs_summary: String,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ContainerStat {
    pub(super) name: String,
    pub(super) cpu_percent: f32,
    pub(super) mem_usage: String,
    pub(super) block_io: String,
}

#[derive(Debug, Clone)]
pub(super) struct ProxyNetworkDrift {
    pub(super) proxy_networks: Vec<String>,
    pub(super) workload_networks: Vec<String>,
    pub(super) missing_networks: Vec<String>,
}
