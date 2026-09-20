/*
 * MCP Tools — despacho: routers call_tool/despachar_* por dominio.
 * (split 119A-3 de tools.rs; definiciones de wire en definiciones.rs.)
 */

use crate::error::CoolifyError;

use serde_json::Value;
use std::path::{Path, PathBuf};
async fn despachar_infra(
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
            crate::commands::minecraft::execute(
                config_path,
                &action,
                &server_name,
                &memory,
                max_players,
                &difficulty,
                "LATEST",
                25565,
                console_cmd.as_deref(),
                lines,
            )
            .await?;
            Ok(format!("Minecraft '{server_name}': {action}"))
        }

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

async fn despachar_diagnostico(
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
            "Tool '{name}' no es de diagnostico"
        ))),
    }
}

async fn despachar_bd(
    config_path: &Path,
    name: &str,
    args: &Value,
) -> std::result::Result<String, CoolifyError> {
    match name {
        "coolify_import_db" => {
            let site_name = get_str(args, "site_name")?;
            let sql_file = get_str(args, "sql_file_path")?;
            let fix_urls = get_bool(args, "fix_urls");
            crate::commands::import_database::execute(
                config_path,
                &site_name,
                &PathBuf::from(&sql_file),
                fix_urls,
            )
            .await?;
            Ok(format!("Base de datos importada en '{site_name}'"))
        }

        "coolify_export_db" => {
            let site_name = get_str(args, "site_name")?;
            let output = args
                .get("output_path")
                .and_then(|v| v.as_str())
                .map(PathBuf::from);
            crate::commands::export_database::execute(config_path, &site_name, output.as_deref())
                .await?;
            Ok(format!("Base de datos exportada de '{site_name}'"))
        }

        "coolify_backup" => {
            let site_name = get_str(args, "site_name")?;
            let tier = get_str_or(args, "tier", "manual");
            let label = get_opt_str(args, "label");
            let list = get_bool(args, "list");
            crate::commands::backup_site::execute(
                config_path,
                &site_name,
                &tier,
                label.as_deref(),
                list,
            )
            .await?;
            Ok(if list {
                format!("Backups listados para '{site_name}'")
            } else {
                format!("Backup creado para '{site_name}'")
            })
        }

        "coolify_restore_backup" => {
            let site_name = get_str(args, "site_name")?;
            let backup_id = get_str(args, "backup_id")?;
            let skip_safety_snapshot = get_bool(args, "skip_safety_snapshot");
            crate::commands::restore_backup::execute(
                config_path,
                &site_name,
                &backup_id,
                skip_safety_snapshot,
            )
            .await?;
            Ok(format!("Backup '{backup_id}' restaurado en '{site_name}'"))
        }

        "coolify_migrate" => {
            let site_name = get_str(args, "site_name")?;
            let target = get_str(args, "target")?;
            let dry_run = get_bool(args, "dry_run");
            let switch_dns = get_bool(args, "switch_dns");
            crate::commands::migrate_site::execute(
                config_path,
                &site_name,
                &target,
                dry_run,
                switch_dns,
            )
            .await?;
            Ok(format!(
                "Migracion ejecutada para '{site_name}' hacia '{target}'"
            ))
        }

        "coolify_db_compare" => {
            let site_name = get_str(args, "site_name")?;
            let dump = get_opt_str(args, "dump");
            let against = get_opt_str(args, "against");
            let tables = get_opt_str(args, "tables");
            let ignore_columns = get_opt_str(args, "ignore_columns");
            let limit_diff = args
                .get("limit_diff")
                .and_then(|v| v.as_u64())
                .unwrap_or(20) as usize;
            let no_tmp_container = get_bool(args, "no_tmp_container");
            let extract_limit = args.get("extract_limit").and_then(|v| v.as_u64());
            let json = crate::commands::db_compare::execute_json(
                config_path,
                &site_name,
                dump,
                against,
                tables,
                ignore_columns,
                limit_diff,
                no_tmp_container,
                extract_limit,
            )
            .await?;
            Ok(json)
        }

        _ => Err(CoolifyError::Validation(format!(
            "Tool '{name}' no es de BD"
        ))),
    }
}

/* [119A-3] Despacho por dominio: cada grupo contiene sus brazos del match
 * movidos verbatim desde `call_tool`. `call_tool` queda como dispatcher fino. */
async fn despachar_sitios(
    config_path: &Path,
    name: &str,
    args: &Value,
) -> std::result::Result<String, CoolifyError> {
    match name {
        "coolify_new_site" => {
            let site_name = get_str(args, "site_name")?;
            let domain = get_str(args, "domain")?;
            let glory_branch = get_str_or(args, "glory_branch", "main");
            let library_branch = get_str_or(args, "library_branch", "main");
            let template = get_str_or(args, "template", "wordpress");
            let target = get_opt_str(args, "target");
            /* [268A-5] Parámetros opcionales del stack Rust para proyectos no-glory */
            let repo_url = args.get("repo_url").and_then(|v| v.as_str());
            let app_bin = args.get("app_bin").and_then(|v| v.as_str());
            let frontend_dir = args.get("frontend_dir").and_then(|v| v.as_str());
            /* [119A-4] Imagen precompilada opcional (registry/owner/app:tag) */
            let image = args.get("image").and_then(|v| v.as_str());
            let skip_theme = get_bool(args, "skip_theme");
            let skip_cache = get_bool(args, "skip_cache");

            crate::commands::new_site::execute(
                config_path,
                &site_name,
                &domain,
                &glory_branch,
                &library_branch,
                &template,
                target.as_deref(),
                repo_url,
                app_bin,
                frontend_dir,
                image,
                skip_theme,
                skip_cache,
            )
            .await?;
            Ok(format!(
                "Sitio '{site_name}' creado exitosamente en {domain}"
            ))
        }

        "coolify_deploy_theme" => {
            let site_name = get_str(args, "site_name")?;
            let glory_branch = get_opt_str(args, "glory_branch");
            let library_branch = get_opt_str(args, "library_branch");
            let update = get_bool(args, "update");
            let skip_react = get_bool(args, "skip_react");
            let force = get_bool(args, "force");
            let skip_backup = get_bool(args, "skip_backup");

            crate::commands::deploy_theme::execute(
                config_path,
                &site_name,
                glory_branch.as_deref(),
                library_branch.as_deref(),
                update,
                skip_react,
                force,
                skip_backup,
            )
            .await?;
            Ok(format!("Tema desplegado en '{site_name}'"))
        }

        "coolify_list_sites" => {
            let detailed = get_bool(args, "detailed");
            crate::commands::list_sites::execute(config_path, detailed).await?;
            Ok("Lista de sitios mostrada".to_string())
        }

        "coolify_restart" => {
            let site_name = get_opt_str(args, "site_name");
            let all = get_bool(args, "all");
            let only_db = get_bool(args, "only_db");
            let only_wordpress = get_bool(args, "only_wordpress");
            crate::commands::restart_site::execute(
                config_path,
                site_name.as_deref(),
                all,
                only_db,
                only_wordpress,
            )
            .await?;
            Ok("Servicio(s) reiniciado(s)".to_string())
        }

        "coolify_set_domain" => {
            let site_name = get_str(args, "site_name")?;
            let new_domain = get_str(args, "new_domain")?;
            crate::commands::set_domain::execute(config_path, &site_name, &new_domain).await?;
            Ok(format!("Dominio actualizado a '{new_domain}'"))
        }

        "coolify_redeploy" => {
            let site_name = get_str(args, "site_name")?;
            crate::commands::redeploy::execute(config_path, &site_name, false).await?;
            Ok(format!("Redeploy iniciado para '{site_name}'"))
        }

        "coolify_deploy_websocket" => {
            let site_name = get_str(args, "site_name")?;
            crate::commands::deploy_websocket::execute(config_path, &site_name).await?;
            Ok(format!("WebSocket desplegado en '{site_name}'"))
        }

        _ => Err(CoolifyError::Validation(format!(
            "Tool '{name}' no es de sitios"
        ))),
    }
}

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

/* Helpers para extraer valores de args JSON */

/// [119A-3] Extracción opcional de string (evita repetir el trío
/// `get().and_then(as_str).map(to_string)` en cada dispatcher).
fn get_opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn get_str(args: &Value, key: &str) -> std::result::Result<String, CoolifyError> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| CoolifyError::Validation(format!("Parametro requerido: '{key}'")))
}

fn get_str_or(args: &Value, key: &str, default: &str) -> String {
    args.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(default)
        .to_string()
}

fn get_bool(args: &Value, key: &str) -> bool {
    args.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}
