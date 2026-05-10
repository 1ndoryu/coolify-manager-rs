/*
 * Tipos de respuesta de la API publica.
 * Todos serializables con serde para consumo desde Tauri/frontend.
 */

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SiteSummary {
    pub name: String,
    pub domain: String,
    pub target: String,
    pub stack_uuid: String,
    pub template: String,
}

#[derive(Debug, Serialize)]
pub struct MinecraftSummary {
    pub name: String,
    pub memory: String,
    pub max_players: u32,
}

#[derive(Debug, Serialize)]
pub struct SitesResponse {
    pub sites: Vec<SiteSummary>,
    pub minecraft: Vec<MinecraftSummary>,
}

#[derive(Debug, Serialize)]
pub struct TargetSummary {
    pub name: String,
    pub host: String,
    pub user: String,
    pub coolify_url: String,
    pub site_count: usize,
}

#[derive(Debug, Serialize)]
pub struct TargetsResponse {
    pub default_target: String,
    pub config_path: String,
    pub targets: Vec<TargetSummary>,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub site_name: String,
    pub url: String,
    pub http_ok: bool,
    pub app_ok: bool,
    pub fatal_log_detected: bool,
    pub status_code: Option<u16>,
    pub healthy: bool,
    pub details: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BackupSummary {
    pub backup_id: String,
    pub tier: String,
    pub status: String,
    pub created_at: String,
    pub label: Option<String>,
    pub artifact_count: usize,
}

#[derive(Debug, Serialize)]
pub struct BackupsResponse {
    pub site_name: String,
    pub backups: Vec<BackupSummary>,
}

#[derive(Debug, Serialize)]
pub struct BackupOverviewSummary {
    pub site_name: String,
    pub domain: String,
    pub target: String,
    pub template: String,
    pub backup_id: String,
    pub tier: String,
    pub status: String,
    pub created_at: String,
    pub label: Option<String>,
    pub artifact_count: usize,
}

#[derive(Debug, Serialize)]
pub struct BackupListError {
    pub site_name: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct BackupsOverviewResponse {
    pub backups: Vec<BackupOverviewSummary>,
    pub errors: Vec<BackupListError>,
}

#[derive(Debug, Serialize)]
pub struct AuditResponse {
    pub target: String,
    pub load_average: String,
    pub memory_summary: String,
    pub disk_summary: String,
    pub docker_summary: String,
    pub security_summary: String,
    pub recommendations: Vec<String>,
    pub load_1m: Option<f32>,
    pub load_5m: Option<f32>,
    pub load_15m: Option<f32>,
    pub memory_used_mb: Option<u64>,
    pub memory_free_mb: Option<u64>,
    pub memory_total_mb: Option<u64>,
    pub disk_use_percent: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct ContainerMetric {
    pub name: String,
    pub cpu_percent: f32,
    pub memory_usage: String,
    pub memory_percent: f32,
    pub memory_used_bytes: u64,
    pub memory_limit_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct DeploymentMetric {
    pub site_name: String,
    pub target: String,
    pub status: String,
    pub total_cpu_percent: f32,
    pub memory_used_bytes: u64,
    pub memory_limit_bytes: u64,
    pub memory_percent: f32,
    pub containers: Vec<ContainerMetric>,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct DeploymentMetricsResponse {
    pub generated_at: String,
    pub metrics: Vec<DeploymentMetric>,
}

#[derive(Debug, Serialize)]
pub struct LogsResponse {
    pub site_name: String,
    pub container_target: String,
    pub lines: u32,
    pub content: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Debug, Serialize)]
pub struct OperationResult {
    pub success: bool,
    pub message: String,
    pub details: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EnvVarDiff {
    pub key: String,
    pub local: Option<String>,
    pub remote: Option<String>,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct EnvSyncReport {
    pub site_name: String,
    pub local_count: usize,
    pub remote_count: usize,
    pub diff_count: usize,
    pub diffs: Vec<EnvVarDiff>,
    pub applied: bool,
    pub dry_run: bool,
}
