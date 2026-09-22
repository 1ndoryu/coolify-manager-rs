/* Split 119A-5 de cli/dispatch/ops.rs — comandos de plataforma Coolify.
 * Código verbatim del original; solo cambia visibilidad a pub(super). */
use super::ligero;
use crate::cli::Command;

use coolify_manager::commands;
use coolify_manager::error::CoolifyError;

use std::path::Path;

/* [245A-9] El dispatcher central del manager sigue concentrando muchos comandos.
 * En este bloque solo se incorporan light-backup/light-restore. */
// sentinel-disable-next-line limite-lineas
pub(super) async fn dispatch_coolify_platform_ops(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        Command::Audit { target } => {
            commands::audit_vps::execute(config_path, target.as_deref()).await
        }
        Command::AuditControlPlane {
            target,
            since,
            repair,
        } => {
            commands::audit_control_plane::execute(config_path, target.as_deref(), &since, repair)
                .await
        }
        Command::AuditSecurity { target } => {
            commands::audit_security::execute(config_path, target.as_deref()).await
        }
        Command::AuditRedisLatency {
            target,
            slowlog_count,
        } => {
            commands::audit_redis_latency::execute(config_path, target.as_deref(), slowlog_count)
                .await
        }
        Command::CoolifyControlPlane {
            target,
            action,
            include_proxy,
        } => {
            commands::coolify_control_plane::execute(config_path, &target, &action, include_proxy)
                .await
        }
        Command::InstallCoolify { target } => {
            commands::install_coolify::execute(config_path, &target).await
        }
        command @ (Command::BootstrapTargetLight { .. }
        | Command::ProvisionStatic { .. }
        | Command::InventoryLight { .. }
        | Command::LightBackup { .. }
        | Command::LightRestore { .. }
        | Command::LightSite { .. }) => {
            ligero::dispatch_lightweight_platform_ops(command, config_path).await
        }
        Command::UninstallCoolify {
            target,
            purge_data,
            dry_run,
        } => commands::uninstall_coolify::execute(config_path, &target, purge_data, dry_run).await,
        Command::PurgeDockerHost {
            target,
            all_data,
            dry_run,
        } => commands::purge_docker_host::execute(config_path, &target, all_data, dry_run).await,
        _ => unreachable!("grupo coolify platform ops invalido"),
    }
}
