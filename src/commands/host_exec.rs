/*
 * host-exec — Ejecuta un comando directamente en el host del VPS via SSH.
 *
 * Util para operaciones que no estan cubiertas por comandos especificos:
 * limpiar volumenes Docker, inspeccionar el filesystem del host, etc.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    command: &str,
    target_name: Option<&str>,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;

    let vps_config = match target_name {
        Some(name) => {
            let target = settings.get_target(name)?;
            &target.vps
        }
        None => &settings.vps,
    };

    let mut ssh = SshClient::from_vps(vps_config);
    ssh.connect().await?;

    let result = ssh.execute(command).await?;

    if !result.stdout.is_empty() {
        print!("{}", result.stdout);
    }
    if !result.stderr.is_empty() {
        eprint!("{}", result.stderr);
    }

    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("host-exec fallo (exit {})", result.exit_code),
        });
    }

    Ok(())
}
