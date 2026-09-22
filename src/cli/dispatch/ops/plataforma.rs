/* Split 119A-5 de cli/dispatch/ops.rs — sub-dispatcher de plataforma.
 * Código verbatim del original; solo cambia visibilidad a pub(super). */
use super::{coolify, sitios};
use crate::cli::Command;

use coolify_manager::error::CoolifyError;

use std::path::Path;

pub(super) async fn dispatch_platform_ops(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        command @ (Command::Redeploy { .. }
        | Command::FixDbAuth { .. }
        | Command::DeployWebsocket { .. }
        | Command::RunScript { .. }
        | Command::Smtp { .. }
        | Command::Migrate { .. }
        | Command::SwitchDns { .. }
        | Command::SetupSiteDns { .. }
        | Command::DeleteDns { .. }) => {
            sitios::dispatch_site_platform_ops(command, config_path).await
        }
        command @ (Command::Audit { .. }
        | Command::AuditControlPlane { .. }
        | Command::AuditSecurity { .. }
        | Command::AuditRedisLatency { .. }
        | Command::CoolifyControlPlane { .. }
        | Command::InstallCoolify { .. }
        | Command::BootstrapTargetLight { .. }
        | Command::ProvisionStatic { .. }
        | Command::InventoryLight { .. }
        | Command::LightBackup { .. }
        | Command::LightRestore { .. }
        | Command::LightSite { .. }
        | Command::UninstallCoolify { .. }
        | Command::PurgeDockerHost { .. }) => {
            coolify::dispatch_coolify_platform_ops(command, config_path).await
        }
        _ => unreachable!("grupo platform ops invalido"),
    }
}
