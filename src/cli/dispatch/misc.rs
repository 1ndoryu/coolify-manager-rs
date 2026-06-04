use crate::cli::Command;

use coolify_manager::commands;
use coolify_manager::error::CoolifyError;

use std::path::Path;

pub(super) async fn dispatch_misc_commands(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        Command::List { detailed } => commands::list_sites::execute(config_path, detailed).await,
        Command::Minecraft {
            action,
            server_name,
            memory,
            max_players,
            difficulty,
            version,
            port,
            console_command,
            lines,
        } => {
            commands::minecraft::execute(
                config_path,
                &action,
                &server_name,
                &memory,
                max_players,
                &difficulty,
                &version,
                port,
                console_command.as_deref(),
                lines,
            )
            .await
        }
        Command::AuthDrive => commands::auth_drive::execute(config_path).await,
        Command::ScheduleBackup { name, remove } => {
            commands::schedule_backup::execute(config_path, name.as_deref(), remove).await
        }
        Command::Failover {
            name,
            target,
            backup_id,
            switch_dns,
            skip_provision,
        } => {
            commands::failover::execute(
                config_path,
                &name,
                &target,
                backup_id.as_deref(),
                switch_dns,
                skip_provision,
            )
            .await
        }
        Command::SyncEnv {
            name,
            direction,
            dry_run,
            env_file,
            only,
        } => {
            commands::sync_env::execute(
                config_path,
                &name,
                &direction,
                dry_run,
                env_file.as_deref(),
                &only,
            )
            .await
        }
        Command::GetConfigPath => {
            println!("{}", config_path.display());
            Ok(())
        }
        Command::GuiApi { bind } => {
            coolify_manager::gui_api::run(config_path.to_path_buf(), bind).await
        }
        _ => unreachable!("grupo misc invalido"),
    }
}
