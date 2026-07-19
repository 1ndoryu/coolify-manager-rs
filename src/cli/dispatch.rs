use super::{Cli, Command};

use coolify_manager::error::CoolifyError;

use std::path::Path;

mod deploy;
mod misc;
mod ops;
mod site;

use deploy::dispatch_deploy_commands;
use misc::dispatch_misc_commands;
use ops::dispatch_ops_commands;
use site::dispatch_site_commands;

/// Punto de entrada del CLI — enruta al handler correspondiente.
pub async fn run(cli: Cli) -> std::result::Result<(), CoolifyError> {
    let config_path = coolify_manager::config::Settings::resolve_config_path(cli.config.as_deref());
    dispatch_command(cli.command, &config_path).await
}

async fn dispatch_command(
    command: Option<Command>,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        Some(
            command @ (Command::New { .. }
            | Command::Deploy { .. }
            | Command::DeployService { .. }
            | Command::Restart { .. }
            | Command::Backup { .. }
            | Command::Restore { .. }
            | Command::RestorePgData { .. }
            | Command::Health { .. }),
        ) => dispatch_deploy_commands(command, config_path).await,
        Some(
            command @ (Command::Import { .. }
            | Command::Export { .. }
            | Command::WpSecurity { .. }
            | Command::Exec { .. }
            | Command::Logs { .. }
            | Command::Debug { .. }
            | Command::Cache { .. }
            | Command::GitStatus { .. }
            | Command::SetDomain { .. }
            | Command::Diagnose { .. }),
        ) => dispatch_site_commands(command, config_path).await,
        Some(
            command @ (Command::Redeploy { .. }
            | Command::FixDbAuth { .. }
            | Command::DeployWebsocket { .. }
            | Command::RunScript { .. }
            | Command::Smtp { .. }
            | Command::Migrate { .. }
            | Command::SwitchDns { .. }
            | Command::SetupSiteDns { .. }
            | Command::Audit { .. }
            | Command::AuditControlPlane { .. }
            | Command::AuditSecurity { .. }
            | Command::AuditRedisLatency { .. }
            | Command::CoolifyControlPlane { .. }
            | Command::HardenSsh { .. }
            | Command::EnforceHostSecurity { .. }
            | Command::InstallCoolify { .. }
            | Command::UninstallCoolify { .. }
            | Command::PurgeDockerHost { .. }
            | Command::HostExec { .. }
            | Command::Tailscale { .. }
            | Command::OptimizeHost { .. }
            | Command::MaintainHost { .. }
            | Command::CheckMaintenanceWindow { .. }
            | Command::ScheduleMaintenance { .. }
            | Command::InstallBackups { .. }),
        ) => dispatch_ops_commands(command, config_path).await,
        Some(command) => dispatch_misc_commands(command, config_path).await,
        None => Ok(()),
    }
}
