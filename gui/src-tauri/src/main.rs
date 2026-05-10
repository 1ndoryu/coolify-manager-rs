/*
 * Coolify Manager GUI — entrada Tauri v2.
 * Los comandos Tauri envuelven la API de la libreria.
 */

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use coolify_manager::api;
use coolify_manager::api::types::*;
use coolify_manager::config::Settings;

fn config_path() -> std::path::PathBuf {
    Settings::resolve_config_path(None)
}

#[tauri::command]
async fn list_sites() -> Result<SitesResponse, String> {
    api::list_sites(&config_path())
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn list_targets() -> Result<TargetsResponse, String> {
    api::list_targets(&config_path())
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn health_check(site_name: String) -> Result<HealthResponse, String> {
    api::health_check(&config_path(), &site_name)
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn list_backups(site_name: String) -> Result<BackupsResponse, String> {
    api::list_backups(&config_path(), &site_name)
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn list_all_backups() -> Result<BackupsOverviewResponse, String> {
    api::list_all_backups(&config_path())
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn audit_vps(target: Option<String>) -> Result<AuditResponse, String> {
    api::audit_vps(&config_path(), target.as_deref())
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn deployment_metrics() -> Result<DeploymentMetricsResponse, String> {
    api::deployment_metrics(&config_path())
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn create_site(
    name: String,
    domain: String,
    template: String,
    target: Option<String>,
    skip_theme: Option<bool>,
    skip_cache: Option<bool>,
) -> Result<OperationResult, String> {
    api::create_site(
        &config_path(),
        CreateSiteRequest {
            name,
            domain,
            template,
            target,
            skip_theme: skip_theme.unwrap_or(false),
            skip_cache: skip_cache.unwrap_or(false),
        },
    )
    .await
    .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn view_logs(
    site_name: String,
    lines: Option<u32>,
    container_target: Option<String>,
) -> Result<LogsResponse, String> {
    api::view_logs(
        &config_path(),
        &site_name,
        lines.unwrap_or(120),
        container_target.as_deref(),
    )
    .await
    .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn manual_backup(site_name: String) -> Result<OperationResult, String> {
    api::manual_backup(&config_path(), &site_name)
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn restart_site(site_name: String) -> Result<OperationResult, String> {
    api::restart_site(&config_path(), &site_name)
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn redeploy_site(site_name: String) -> Result<OperationResult, String> {
    api::redeploy_site(&config_path(), &site_name)
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn get_config_path() -> String {
    config_path().to_string_lossy().to_string()
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            list_sites,
            list_targets,
            health_check,
            list_backups,
            list_all_backups,
            audit_vps,
            deployment_metrics,
            create_site,
            view_logs,
            manual_backup,
            restart_site,
            redeploy_site,
            get_config_path,
        ])
        .run(tauri::generate_context!())
        .expect("Error ejecutando aplicacion Tauri");
}
