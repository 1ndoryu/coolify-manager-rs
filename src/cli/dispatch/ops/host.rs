/* Split 119A-5 de cli/dispatch/ops.rs — comandos de mantenimiento del host.
 * Código verbatim del original; solo cambia visibilidad a pub(super). */
use crate::cli::Command;

use coolify_manager::commands;
use coolify_manager::error::CoolifyError;

use std::path::Path;

pub(super) async fn dispatch_host_ops(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        Command::OptimizeHost {
            target,
            swap_gb,
            swappiness,
            vfs_cache_pressure,
            overcommit_memory,
            disable_thp,
            docker_live_restore,
            dry_run,
            samples,
            interval_seconds,
        } => {
            commands::optimize_host::execute(
                config_path,
                target.as_deref(),
                swap_gb,
                swappiness,
                vfs_cache_pressure,
                overcommit_memory,
                disable_thp,
                docker_live_restore,
                dry_run,
                samples,
                interval_seconds,
            )
            .await
        }
        Command::MaintainHost {
            target,
            reboot,
            dry_run,
        } => {
            commands::maintain_host::execute(config_path, target.as_deref(), reboot, dry_run).await
        }
        Command::CheckMaintenanceWindow {
            target,
            apply,
            dry_run,
            force_evaluate,
        } => {
            commands::check_maintenance_window::execute(
                config_path,
                target.as_deref(),
                apply,
                dry_run,
                force_evaluate,
            )
            .await
        }
        Command::ScheduleMaintenance {
            target,
            dry_run,
            remove,
        } => commands::schedule_maintenance::execute(config_path, &target, dry_run, remove).await,
        _ => unreachable!("grupo host ops invalido"),
    }
}
