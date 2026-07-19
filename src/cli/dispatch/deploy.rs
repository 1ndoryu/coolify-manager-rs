use crate::cli::Command;

use coolify_manager::commands;
use coolify_manager::error::CoolifyError;

use std::path::Path;

pub(super) async fn dispatch_deploy_commands(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        Command::New {
            name,
            domain,
            glory_branch,
            library_branch,
            template,
            target,
            skip_theme,
            skip_cache,
        } => {
            commands::new_site::execute(
                config_path,
                &name,
                &domain,
                &glory_branch,
                &library_branch,
                &template,
                target.as_deref(),
                skip_theme,
                skip_cache,
            )
            .await
        }
        Command::Deploy {
            name,
            glory_branch,
            library_branch,
            update,
            skip_react,
            force,
            skip_backup,
        } => {
            commands::deploy_theme::execute(
                config_path,
                &name,
                glory_branch.as_deref(),
                library_branch.as_deref(),
                update,
                skip_react,
                force,
                skip_backup,
            )
            .await
        }
        Command::DeployService {
            name,
            skip_build,
            seed,
            skip_compose_sync,
            skip_backup,
        } => {
            commands::deploy_service::execute(
                config_path,
                &name,
                skip_build,
                seed,
                skip_compose_sync,
                skip_backup,
            )
            .await
        }
        Command::Restart {
            name,
            all,
            only_db,
            only_wordpress,
        } => {
            commands::restart_site::execute(
                config_path,
                name.as_deref(),
                all,
                only_db,
                only_wordpress,
            )
            .await
        }
        Command::Backup {
            name,
            tier,
            label,
            list,
        } => {
            commands::backup_site::execute(config_path, &name, &tier, label.as_deref(), list).await
        }
        Command::Restore {
            name,
            backup_id,
            skip_safety_snapshot,
        } => {
            commands::restore_backup::execute(config_path, &name, &backup_id, skip_safety_snapshot)
                .await
        }
        Command::RestorePgData {
            name,
            file,
            database,
            skip_safety_snapshot,
        } => {
            commands::restore_pg_data::execute(
                config_path,
                &name,
                &file,
                database.as_deref(),
                skip_safety_snapshot,
            )
            .await
        }
        Command::Health {
            name,
            all,
            alert,
            repair,
        } => {
            commands::health_check::execute(config_path, name.as_deref(), all, alert, repair).await
        }
        _ => unreachable!("grupo deploy invalido"),
    }
}
