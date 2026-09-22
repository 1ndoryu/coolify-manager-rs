/*
 * MCP Tools — despacho/bd: routers de base de datos (transferencia y respaldo).
 * (split 119A-5 de despacho.rs para bajar del limite de 500 lineas;
 * codigo verbatim del original.)
 */

use super::comun::{get_bool, get_opt_str, get_str, get_str_or};
use crate::error::CoolifyError;

use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) async fn despachar_bd(
    config_path: &Path,
    name: &str,
    args: &Value,
) -> std::result::Result<String, CoolifyError> {
    match name {
        "coolify_import_db" | "coolify_export_db" => {
            despachar_bd_transferencia(config_path, name, args).await
        }
        _ => despachar_bd_respaldo(config_path, name, args).await,
    }
}

/* BD transferencia: importar y exportar SQL. */
async fn despachar_bd_transferencia(
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

        _ => Err(CoolifyError::Validation(format!(
            "Tool '{name}' no es de BD transferencia"
        ))),
    }
}

/* BD respaldo: backup, restore, migracion y comparacion. */
async fn despachar_bd_respaldo(
    config_path: &Path,
    name: &str,
    args: &Value,
) -> std::result::Result<String, CoolifyError> {
    match name {
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
                crate::services::compare_manager::CompareOptions {
                    site_name,
                    dump,
                    against,
                    tables,
                    ignore_columns,
                    limit_diff,
                    json: true,
                    no_tmp_container,
                    extract_limit,
                },
            )
            .await?;
            Ok(json)
        }

        _ => Err(CoolifyError::Validation(format!(
            "Tool '{name}' no es de BD"
        ))),
    }
}
