/*
 * MCP Tools — despacho: routers call_tool/despachar_* por dominio.
 * (split 119A-3 de tools.rs; definiciones de wire en definiciones.rs.)
 * (split 119A-5: routers por dominio en despacho/{infra,diagnostico,bd,sitios};
 * helpers de args en despacho/comun. `call_tool` queda como dispatcher fino.)
 */

mod bd;
mod comun;
mod diagnostico;
mod infra;
mod sitios;

use bd::despachar_bd;
use diagnostico::despachar_diagnostico;
use infra::despachar_infra;
use sitios::despachar_sitios;

use crate::error::CoolifyError;

use serde_json::Value;
use std::path::Path;

/// Ejecuta una tool por nombre y retorna el resultado como texto.
pub async fn call_tool(
    config_path: &Path,
    name: &str,
    args: Value,
) -> std::result::Result<String, CoolifyError> {
    let config_path = config_path.to_path_buf();

    match name {
        "coolify_new_site"
        | "coolify_deploy_theme"
        | "coolify_list_sites"
        | "coolify_restart"
        | "coolify_set_domain"
        | "coolify_redeploy"
        | "coolify_deploy_websocket" => despachar_sitios(&config_path, name, &args).await,

        "coolify_import_db"
        | "coolify_export_db"
        | "coolify_backup"
        | "coolify_restore_backup"
        | "coolify_migrate"
        | "coolify_db_compare" => despachar_bd(&config_path, name, &args).await,

        "coolify_audit_vps"
        | "coolify_wp_security"
        | "coolify_health"
        | "coolify_exec"
        | "coolify_view_logs"
        | "coolify_debug"
        | "coolify_cache"
        | "coolify_git_status" => despachar_diagnostico(&config_path, name, &args).await,

        "coolify_switch_dns"
        | "coolify_setup_smtp"
        | "coolify_minecraft"
        | "coolify_failover"
        | "coolify_install_coolify"
        | "coolify_run_script" => despachar_infra(&config_path, name, &args).await,

        _ => Err(CoolifyError::Validation(format!("Tool '{name}' no existe"))),
    }
}
