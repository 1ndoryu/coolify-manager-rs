/* Ejecución remota del auditor (119A-5 split control_plane_audit_manager). */

use super::formato::sh_quote;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

pub(super) async fn exec_trim(
    ssh: &SshClient,
    command: &str,
) -> std::result::Result<String, CoolifyError> {
    let result = ssh.execute(command).await?;
    if !result.stdout.trim().is_empty() {
        return Ok(result.stdout.trim().replace('\n', " "));
    }
    Ok(result.stderr.trim().replace('\n', " "))
}

pub(super) async fn exec_raw_script(
    ssh: &SshClient,
    script: &str,
) -> std::result::Result<String, CoolifyError> {
    exec_raw_script_timeout(ssh, script, 15).await
}

pub(super) async fn exec_raw_script_timeout(
    ssh: &SshClient,
    script: &str,
    timeout_seconds: u32,
) -> std::result::Result<String, CoolifyError> {
    let command = format!("timeout {} sh -lc {}", timeout_seconds, sh_quote(script));
    let result = ssh.execute(&command).await?;
    if !result.stdout.trim().is_empty() {
        return Ok(result.stdout.trim().to_string());
    }
    Ok(result.stderr.trim().to_string())
}
