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
async fn audit_vps(target: Option<String>) -> Result<AuditResponse, String> {
    api::audit_vps(&config_path(), target.as_deref())
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
            health_check,
            list_backups,
            audit_vps,
            get_config_path,
        ])
        .run(tauri::generate_context!())
        .expect("Error ejecutando aplicacion Tauri");
}
