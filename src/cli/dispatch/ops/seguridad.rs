/* Split 119A-5 de cli/dispatch/ops.rs — comandos de seguridad del host.
 * Código verbatim del original; solo cambia visibilidad a pub(super). */
use crate::cli::Command;

use coolify_manager::commands;
use coolify_manager::error::CoolifyError;

use std::path::Path;

pub(super) async fn dispatch_security_ops(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        Command::HardenSsh {
            target,
            dry_run,
            apply,
        } => commands::harden_ssh::execute(config_path, &target, dry_run, apply).await,
        Command::EnforceHostSecurity {
            target,
            dry_run,
            apply,
        } => commands::enforce_host_security::execute(config_path, &target, dry_run, apply).await,
        Command::Tailscale {
            target,
            auth_key,
            auth_key_env,
            hostname,
            advertise_tags,
            accept_dns,
            probe_url,
            probe_method,
            probe_body,
        } => {
            commands::tailscale::execute(&commands::tailscale::ParamsTailscale {
                config_path,
                target_name: target.as_deref(),
                auth_key: auth_key.as_deref(),
                auth_key_env: auth_key_env.as_deref(),
                hostname: hostname.as_deref(),
                advertise_tags: advertise_tags.as_deref(),
                accept_dns,
                probe_url: probe_url.as_deref(),
                probe_method: &probe_method,
                probe_body: probe_body.as_deref(),
            })
            .await
        }
        _ => unreachable!("grupo security ops invalido"),
    }
}
