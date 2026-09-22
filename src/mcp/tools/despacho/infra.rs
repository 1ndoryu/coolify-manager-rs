/*
 * MCP Tools — despacho/infra: routers de infraestructura (red y ops).
 * (split 119A-5 de despacho.rs para bajar del limite de 500 lineas;
 * codigo verbatim del original.)
 */

use super::comun::{get_bool, get_opt_str, get_str, get_str_or};
use crate::error::CoolifyError;

use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) async fn despachar_infra(
    config_path: &Path,
    name: &str,
    args: &Value,
) -> std::result::Result<String, CoolifyError> {
    match name {
        "coolify_switch_dns" | "coolify_setup_smtp" | "coolify_minecraft" => {
            despachar_infra_red(config_path, name, args).await
        }
        "coolify_failover" | "coolify_install_coolify" | "coolify_run_script" => {
            despachar_infra_ops(config_path, name, args).await
        }
        _ => Err(CoolifyError::Validation(format!(
            "Tool '{name}' no es de infra"
        ))),
    }
}

/* Infra red: DNS, SMTP y minecraft. */
async fn despachar_infra_red(
    config_path: &Path,
    name: &str,
    args: &Value,
) -> std::result::Result<String, CoolifyError> {
    match name {
        "coolify_switch_dns" => {
            let site_name = get_str(args, "site_name")?;
            let target = get_opt_str(args, "target");
            let target_ip = get_opt_str(args, "target_ip");
            let dry_run = get_bool(args, "dry_run");
            crate::commands::switch_dns::execute(
                config_path,
                &site_name,
                target.as_deref(),
                target_ip.as_deref(),
                dry_run,
            )
            .await?;
            Ok(format!("DNS conmutado para '{site_name}'"))
        }

        "coolify_setup_smtp" => {
            let site_name = get_opt_str(args, "site_name");
            let all = get_bool(args, "all");
            let test = get_bool(args, "test");
            let test_email = get_opt_str(args, "test_email");
            crate::commands::setup_smtp::execute(
                config_path,
                site_name.as_deref(),
                all,
                test,
                test_email.as_deref(),
                false,
            )
            .await?;
            Ok("SMTP configurado".to_string())
        }

        "coolify_minecraft" => {
            let action = get_str(args, "action")?;
            let server_name = get_str(args, "server_name")?;
            let memory = get_str_or(args, "memory", "2G");
            let max_players = args
                .get("max_players")
                .and_then(|v| v.as_u64())
                .unwrap_or(20) as u32;
            let difficulty = get_str_or(args, "difficulty", "normal");
            let console_cmd = get_opt_str(args, "console_command");
            let lines = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(100) as u32;
            crate::commands::minecraft::execute(&crate::commands::minecraft::ParamsMinecraft {
                config_path,
                action: &action,
                server_name: &server_name,
                memory: &memory,
                max_players,
                difficulty: &difficulty,
                version: "LATEST",
                port: 25565,
                console_command: console_cmd.as_deref(),
                lines,
            })
            .await?;
            Ok(format!("Minecraft '{server_name}': {action}"))
        }

        _ => Err(CoolifyError::Validation(format!(
            "Tool '{name}' no es de infra red"
        ))),
    }
}

/* Infra ops: failover, instalacion Coolify y scripts. */
async fn despachar_infra_ops(
    config_path: &Path,
    name: &str,
    args: &Value,
) -> std::result::Result<String, CoolifyError> {
    match name {
        "coolify_failover" => {
            let site_name = get_str(args, "site_name")?;
            let target = get_str(args, "target")?;
            let backup_id = get_opt_str(args, "backup_id");
            let switch_dns = get_bool(args, "switch_dns");
            let skip_provision = get_bool(args, "skip_provision");
            crate::commands::failover::execute(
                config_path,
                &site_name,
                &target,
                backup_id.as_deref(),
                switch_dns,
                skip_provision,
            )
            .await?;
            Ok(format!("Failover completado: '{site_name}' -> '{target}'"))
        }

        "coolify_install_coolify" => {
            let target = get_str(args, "target")?;
            crate::commands::install_coolify::execute(config_path, &target).await?;
            Ok(format!("Coolify instalado en target '{target}'"))
        }

        "coolify_run_script" => {
            let site_name = get_str(args, "site_name")?;
            let file_path = get_str(args, "file_path")?;
            let interpreter = get_opt_str(args, "interpreter");
            let target = get_str_or(args, "target", "wordpress");
            let script_args = get_opt_str(args, "args");
            crate::commands::run_script::execute(
                config_path,
                &site_name,
                &PathBuf::from(&file_path),
                interpreter.as_deref(),
                &target,
                script_args.as_deref(),
            )
            .await?;
            Ok(format!("Script ejecutado en '{site_name}'"))
        }

        _ => Err(CoolifyError::Validation(format!(
            "Tool '{name}' no es de infra"
        ))),
    }
}
