/* Split 119A-5 de cli/dispatch/ops.rs — comandos de plataforma por sitio.
 * Código verbatim del original; solo cambia visibilidad a pub(super). */
use crate::cli::Command;

use coolify_manager::commands;
use coolify_manager::error::CoolifyError;

use std::path::Path;

pub(super) async fn dispatch_site_platform_ops(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        Command::Redeploy { name, skip_backup } => {
            commands::redeploy::execute(config_path, &name, skip_backup).await
        }
        Command::FixDbAuth { name, dry_run } => {
            commands::fix_db_auth::execute(config_path, &name, dry_run).await
        }
        Command::DeployWebsocket { name } => {
            commands::deploy_websocket::execute(config_path, &name).await
        }
        Command::RunScript {
            name,
            file,
            interpreter,
            target,
            args,
        } => {
            commands::run_script::execute(
                config_path,
                &name,
                &file,
                interpreter.as_deref(),
                &target,
                args.as_deref(),
            )
            .await
        }
        Command::Smtp {
            name,
            all,
            test,
            test_email,
            status,
        } => {
            commands::setup_smtp::execute(
                config_path,
                name.as_deref(),
                all,
                test,
                test_email.as_deref(),
                status,
            )
            .await
        }
        Command::Migrate {
            name,
            target,
            dry_run,
            switch_dns,
        } => {
            commands::migrate_site::execute(config_path, &name, &target, dry_run, switch_dns).await
        }
        Command::SwitchDns {
            name,
            target,
            ip,
            dry_run,
        } => {
            commands::switch_dns::execute(
                config_path,
                &name,
                target.as_deref(),
                ip.as_deref(),
                dry_run,
            )
            .await
        }
        /* [156A-1] setup-site-dns: configura DNS completo + verifica HTTPS */
        Command::SetupSiteDns {
            name,
            ip,
            dry_run,
            skip_verify,
        } => {
            let settings = coolify_manager::config::Settings::load(config_path)?;
            let args = commands::setup_site_dns::SetupSiteDnsArgs {
                name,
                ip,
                dry_run,
                skip_verify,
            };
            commands::setup_site_dns::run(&settings, &args).await
        }
        /* [B4-4] delete-dns: retira un registro huérfano con confirmación tipada */
        Command::DeleteDns {
            provider,
            zone,
            name,
            ip,
            confirm,
            dry_run,
        } => {
            let args = commands::delete_dns::DeleteDnsArgs {
                provider,
                zone,
                name,
                ip,
                confirm,
                dry_run,
            };
            commands::delete_dns::run(config_path, &args).await
        }
        _ => unreachable!("grupo site platform ops invalido"),
    }
}
