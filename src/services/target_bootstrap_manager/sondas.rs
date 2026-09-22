/* Split 119A-5 de target_bootstrap_manager.rs — sondas SSH reutilizables.
 * Código verbatim del original; solo cambia visibilidad a pub(super). */

use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

pub(super) async fn list_items(
    ssh: &SshClient,
    command: &str,
) -> std::result::Result<Vec<String>, CoolifyError> {
    let output = ssh.execute(command).await?;
    Ok(output
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect())
}

pub(super) async fn command_exists(
    ssh: &SshClient,
    command_name: &str,
) -> std::result::Result<bool, CoolifyError> {
    let result = ssh
        .execute(&format!(
            "bash -lc 'command -v {} >/dev/null 2>&1 && echo yes || echo no'",
            command_name
        ))
        .await?;
    Ok(result.stdout.trim() == "yes")
}

pub(super) async fn command_exists_any(
    ssh: &SshClient,
    command_names: &[&str],
) -> std::result::Result<bool, CoolifyError> {
    for command_name in command_names {
        if command_exists(ssh, command_name).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) async fn service_active(
    ssh: &SshClient,
    service_name: &str,
) -> std::result::Result<bool, CoolifyError> {
    let result = ssh
        .execute(&format!(
            "bash -lc 'systemctl is-active {} >/dev/null 2>&1 && echo yes || echo no'",
            service_name
        ))
        .await?;
    Ok(result.stdout.trim() == "yes")
}

pub(super) async fn path_exists(
    ssh: &SshClient,
    path: &str,
) -> std::result::Result<bool, CoolifyError> {
    let result = ssh
        .execute(&format!(
            "bash -lc 'test -e {} && echo yes || echo no'",
            shell_escape(path)
        ))
        .await?;
    Ok(result.stdout.trim() == "yes")
}

pub(super) fn describe_items(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(",")
    }
}

pub(super) fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) fn shell_escape(value: &str) -> String {
    sh_quote(value)
}
