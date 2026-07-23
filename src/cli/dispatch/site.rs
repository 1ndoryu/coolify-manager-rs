use crate::cli::Command;

use coolify_manager::commands;
use coolify_manager::error::CoolifyError;

use std::path::Path;

pub(super) async fn dispatch_site_commands(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        Command::Import {
            name,
            file,
            fix_urls,
        } => commands::import_database::execute(config_path, &name, &file, fix_urls).await,
        Command::Export { name, output } => {
            commands::export_database::execute(config_path, &name, output.as_deref()).await
        }
        Command::WpSecurity {
            name,
            audit,
            user,
            password,
        } => {
            commands::wordpress_security::execute(
                config_path,
                &name,
                audit,
                user.as_deref(),
                password.as_deref(),
            )
            .await
        }
        Command::Exec {
            name,
            command,
            php,
            target,
        } => {
            commands::exec_command::execute(
                config_path,
                &name,
                command.as_deref(),
                php.as_deref(),
                &target,
            )
            .await
        }
        Command::Logs {
            name,
            lines,
            target,
            wp_debug,
            filter,
            docker_socket,
        } => {
            commands::view_logs::execute(
                config_path,
                &name,
                lines,
                &target,
                wp_debug,
                filter.as_deref(),
                docker_socket.as_deref(),
            )
            .await
        }
        Command::Debug {
            name,
            enable,
            disable,
            status,
        } => commands::debug_site::execute(config_path, &name, enable, disable, status).await,
        Command::Cache { name, action, all } => {
            commands::cache_site::execute(config_path, name.as_deref(), &action, all).await
        }
        Command::GitStatus { name } => commands::git_status::execute(config_path, &name).await,
        Command::SetDomain { name, domain } => {
            commands::set_domain::execute(config_path, &name, &domain).await
        }
        Command::Diagnose { name, json } => {
            commands::diagnose::execute(config_path, &name, json).await
        }
        _ => unreachable!("grupo site invalido"),
    }
}
