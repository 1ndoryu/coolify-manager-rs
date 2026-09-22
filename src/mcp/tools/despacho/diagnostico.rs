/*
 * MCP Tools — despacho/diagnostico: routers de diagnostico (salud y runtime).
 * (split 119A-5 de despacho.rs para bajar del limite de 500 lineas;
 * codigo verbatim del original.)
 */

use super::comun::{get_bool, get_opt_str, get_str, get_str_or};
use crate::error::CoolifyError;

use serde_json::Value;
use std::path::Path;

pub(crate) async fn despachar_diagnostico(
    config_path: &Path,
    name: &str,
    args: &Value,
) -> std::result::Result<String, CoolifyError> {
    match name {
        "coolify_health" | "coolify_audit_vps" | "coolify_wp_security" => {
            despachar_diagnostico_salud(config_path, name, args).await
        }
        _ => despachar_diagnostico_runtime(config_path, name, args).await,
    }
}

/* Diagnostico salud: health, auditoria VPS y seguridad WP. */
async fn despachar_diagnostico_salud(
    config_path: &Path,
    name: &str,
    args: &Value,
) -> std::result::Result<String, CoolifyError> {
    match name {
        "coolify_health" => {
            let site_name = get_str(args, "site_name")?;
            let alert = get_bool(args, "alert");
            let repair = get_bool(args, "repair");
            crate::commands::health_check::execute(
                config_path,
                Some(&site_name),
                false,
                alert,
                repair,
            )
            .await?;
            Ok(format!("Health check ejecutado para '{site_name}'"))
        }

        "coolify_audit_vps" => {
            let target = get_opt_str(args, "target");
            crate::commands::audit_vps::execute(config_path, target.as_deref()).await?;
            Ok("Auditoria VPS completada".to_string())
        }

        "coolify_wp_security" => {
            let site_name = get_str(args, "site_name")?;
            let audit = args.get("audit").and_then(|v| v.as_bool()).unwrap_or(true);
            let user = get_opt_str(args, "user");
            let password = get_opt_str(args, "password");
            crate::commands::wordpress_security::execute(
                config_path,
                &site_name,
                audit,
                user.as_deref(),
                password.as_deref(),
            )
            .await?;
            Ok(format!("Auditoria WordPress completada para '{site_name}'"))
        }

        _ => Err(CoolifyError::Validation(format!(
            "Tool '{name}' no es de diagnostico salud"
        ))),
    }
}

/* Diagnostico runtime: exec, logs, debug, cache y git status. */
async fn despachar_diagnostico_runtime(
    config_path: &Path,
    name: &str,
    args: &Value,
) -> std::result::Result<String, CoolifyError> {
    match name {
        "coolify_exec" => {
            let site_name = get_str(args, "site_name")?;
            let command = get_opt_str(args, "command");
            let php_code = get_opt_str(args, "php_code");
            let target = get_str_or(args, "target", "wordpress");
            crate::commands::exec_command::execute(
                config_path,
                &site_name,
                command.as_deref(),
                php_code.as_deref(),
                &target,
            )
            .await?;
            Ok("Comando ejecutado".to_string())
        }

        /* [257B-1] Actualizado para pasar since, until, pattern */
        "coolify_view_logs" => {
            let site_name = get_str(args, "site_name")?;
            let lines = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(50) as u32;
            let target = get_str_or(args, "target", "wordpress");
            let wp_debug = get_bool(args, "wp_debug");
            let filter = get_opt_str(args, "filter");
            let docker_socket = get_opt_str(args, "docker_socket");
            let since = get_opt_str(args, "since");
            let until = get_opt_str(args, "until");
            let pattern = get_opt_str(args, "pattern");

            crate::commands::view_logs::execute(
                config_path,
                &site_name,
                lines,
                &target,
                wp_debug,
                filter.as_deref(),
                docker_socket.as_deref(),
                since.as_deref(),
                until.as_deref(),
                pattern.as_deref(),
            )
            .await?;
            Ok("Logs obtenidos".to_string())
        }

        "coolify_debug" => {
            let site_name = get_str(args, "site_name")?;
            let enable = get_bool(args, "enable");
            let disable = get_bool(args, "disable");
            crate::commands::debug_site::execute(
                config_path,
                &site_name,
                enable,
                disable,
                !enable && !disable,
            )
            .await?;
            Ok("WP_DEBUG actualizado".to_string())
        }

        "coolify_cache" => {
            let site_name = get_opt_str(args, "site_name");
            let action = get_str(args, "action")?;
            let all = get_bool(args, "all");
            crate::commands::cache_site::execute(config_path, site_name.as_deref(), &action, all)
                .await?;
            Ok("Cache actualizado".to_string())
        }

        "coolify_git_status" => {
            let site_name = get_str(args, "site_name")?;
            crate::commands::git_status::execute(config_path, &site_name).await?;
            Ok("Estado de Git obtenido".to_string())
        }

        _ => Err(CoolifyError::Validation(format!(
            "Tool '{name}' no es de diagnostico runtime"
        ))),
    }
}
