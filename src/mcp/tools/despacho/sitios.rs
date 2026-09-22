/*
 * MCP Tools — despacho/sitios: routers de sitios (vida y config).
 * (split 119A-5 de despacho.rs para bajar del limite de 500 lineas;
 * codigo verbatim del original.)
 */

use super::comun::{get_bool, get_opt_str, get_str, get_str_or};
use crate::error::CoolifyError;

use serde_json::Value;
use std::path::Path;

/* [119A-3] Despacho por dominio: cada grupo contiene sus brazos del match
 * movidos verbatim desde `call_tool`. `call_tool` queda como dispatcher fino. */
pub(crate) async fn despachar_sitios(
    config_path: &Path,
    name: &str,
    args: &Value,
) -> std::result::Result<String, CoolifyError> {
    match name {
        "coolify_new_site" | "coolify_deploy_theme" | "coolify_list_sites" | "coolify_restart" => {
            despachar_sitios_vida(config_path, name, args).await
        }
        _ => despachar_sitios_config(config_path, name, args).await,
    }
}

/* Sitios vida: crear, tema, listar y reiniciar. */
async fn despachar_sitios_vida(
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

        _ => Err(CoolifyError::Validation(format!(
            "Tool '{name}' no es de sitios vida"
        ))),
    }
}

/* Sitios config: dominio, redeploy y websocket. */
async fn despachar_sitios_config(
    config_path: &Path,
    name: &str,
    args: &Value,
) -> std::result::Result<String, CoolifyError> {
    match name {
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
