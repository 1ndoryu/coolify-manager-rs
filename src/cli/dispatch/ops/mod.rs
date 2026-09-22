/* Dispatcher central de ops CLI (split 119A-5 del antiguo ops.rs).
 * La concentración del switch responde al contrato funcional 245A-9;
 * cada grupo vive en su submódulo y la entrada sigue en
 * dispatch::ops::dispatch_ops_commands sin cambios para dispatch.rs. */
use crate::cli::Command;

use coolify_manager::commands;
use coolify_manager::error::CoolifyError;

use std::path::Path;

mod coolify;
mod host;
mod ligero;
mod plataforma;
mod seguridad;
mod sitios;

pub(super) async fn dispatch_ops_commands(
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
        | Command::DeleteDns { .. }
        | Command::Audit { .. }
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
            plataforma::dispatch_platform_ops(command, config_path).await
        }
        command @ (Command::HardenSsh { .. }
        | Command::EnforceHostSecurity { .. }
        | Command::Tailscale { .. }) => {
            seguridad::dispatch_security_ops(command, config_path).await
        }
        command @ (Command::OptimizeHost { .. }
        | Command::MaintainHost { .. }
        | Command::CheckMaintenanceWindow { .. }
        | Command::ScheduleMaintenance { .. }) => {
            host::dispatch_host_ops(command, config_path).await
        }
        Command::InstallBackups {
            target,
            dry_run,
            uninstall,
        } => {
            commands::install_backups::execute(config_path, target.as_deref(), dry_run, uninstall)
                .await
        }
        Command::HostExec { command, target } => {
            commands::host_exec::execute(config_path, &command, target.as_deref()).await
        }
        /* [119A-4 F2] registry-login: auth Docker VPS contra registry privado */
        Command::RegistryLogin {
            host,
            user,
            target,
            dry_run,
        } => {
            let settings = coolify_manager::config::Settings::load(config_path)?;
            let args = commands::registry_login::RegistryLoginArgs {
                host,
                user,
                target,
                dry_run,
            };
            commands::registry_login::run(&settings, &args).await
        }
        Command::RunSql {
            name,
            query,
            file,
            dry_run,
        } => {
            commands::run_sql::execute(
                config_path,
                &name,
                query.as_deref(),
                file.as_deref(),
                dry_run,
            )
            .await
        }
        Command::DbCheck {
            name,
            expected_tables,
        } => commands::db_check::execute(config_path, &name, expected_tables.as_deref()).await,
        Command::DbMigrate {
            name,
            migrations_dir,
            file,
            dry_run,
        } => {
            commands::db_migrate::execute(
                config_path,
                &name,
                migrations_dir.as_deref(),
                file.as_deref(),
                dry_run,
            )
            .await
        }
        Command::DbCompare {
            name,
            dump,
            against,
            tables,
            ignore_columns,
            limit_diff,
            json,
            no_tmp_container,
            extract_limit,
        } => {
            commands::db_compare::run(
                config_path,
                &name,
                dump,
                against,
                tables,
                ignore_columns,
                limit_diff,
                json,
                no_tmp_container,
                extract_limit,
            )
            .await
        }
        Command::RestoreClient {
            name,
            admin_email,
            admin_password,
            stripe_sub_id,
            dry_run,
        } => {
            commands::restore_client::execute(
                config_path,
                &name,
                &admin_email,
                &admin_password,
                stripe_sub_id.as_deref(),
                dry_run,
            )
            .await
        }
        _ => unreachable!("grupo ops invalido"),
    }
}
