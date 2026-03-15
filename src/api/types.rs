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
pub struct AuditResponse {
    pub target: String,
    pub load_average: String,
    pub memory_summary: String,
    pub disk_summary: String,
    pub docker_summary: String,
    pub security_summary: String,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct OperationResult {
    pub success: bool,
    pub message: String,
    pub details: Option<String>,
}
