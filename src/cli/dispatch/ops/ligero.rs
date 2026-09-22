/* Split 119A-5 de cli/dispatch/ops.rs — comandos del runtime ligero.
 * Código verbatim del original; solo cambia visibilidad a pub(super). */
use crate::cli::Command;

use coolify_manager::commands;
use coolify_manager::error::CoolifyError;

use std::path::Path;

pub(super) async fn dispatch_lightweight_platform_ops(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        Command::BootstrapTargetLight { target, dry_run } => {
            commands::bootstrap_target_light::execute(config_path, &target, dry_run).await
        }
        Command::ProvisionStatic {
            target,
            site,
            fqdn,
            access_user,
            access_password,
            json,
        } => {
            commands::provision_static::execute(
                config_path,
                &target,
                &site,
                fqdn.as_deref(),
                access_user.as_deref(),
                access_password.as_deref(),
                json,
            )
            .await
        }
        Command::InventoryLight { target, json } => {
            commands::inventory_light::execute(config_path, &target, json).await
        }
        Command::LightBackup {
            target,
            site,
            tier,
            label,
            list,
            json,
        } => {
            commands::light_backup::execute(
                config_path,
                &target,
                &site,
                &tier,
                label.as_deref(),
                list,
                json,
            )
            .await
        }
        Command::LightRestore {
            target,
            site,
            backup_id,
            access_password,
            skip_safety_snapshot,
            json,
        } => {
            commands::light_restore::execute(
                config_path,
                &target,
                &site,
                &backup_id,
                access_password.as_deref(),
                skip_safety_snapshot,
                json,
            )
            .await
        }
        Command::LightSite {
            target,
            site,
            action,
            fqdn,
            access_user,
            access_password,
            delete_volumes,
            json,
        } => {
            commands::light_site::execute(&commands::light_site::ParamsLightSite {
                config_path,
                target_name: &target,
                site_name: &site,
                action: &action,
                fqdn: fqdn.as_deref(),
                access_user: access_user.as_deref(),
                access_password: access_password.as_deref(),
                delete_volumes,
                json,
            })
            .await
        }
        _ => unreachable!("grupo lightweight platform ops invalido"),
    }
}
