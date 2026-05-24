use crate::config::{DeploymentTargetConfig, SshSecurityPolicyConfig, VpsConfig};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use serde::Serialize;

const SSH_OVERRIDE_PATH: &str = "/etc/ssh/sshd_config.d/99-coolify-manager.conf";
const CLOUD_INIT_SSH_PATH: &str = "/etc/ssh/sshd_config.d/50-cloud-init.conf";

#[derive(Debug, Clone)]
pub struct HardenSshRequest {
    pub dry_run: bool,
    pub apply: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HardenSshReport {
    pub target: String,
    pub applied: bool,
    pub reconnect_validated: bool,
    pub override_path: String,
    pub backup_path: String,
    pub applied_steps: Vec<String>,
    pub warnings: Vec<String>,
}

pub async fn harden_target(
    target: &DeploymentTargetConfig,
    request: &HardenSshRequest,
) -> std::result::Result<HardenSshReport, CoolifyError> {
    let ssh_policy = target
        .security_policy
        .as_ref()
        .and_then(|policy| policy.ssh.as_ref())
        .ok_or_else(|| {
            CoolifyError::Validation(format!(
                "Target '{}' sin securityPolicy.ssh; no hay politica SSH que aplicar",
                target.name
            ))
        })?;

    let ssh_key = target.vps.ssh_key.as_deref().ok_or_else(|| {
        CoolifyError::Validation(format!(
            "Target '{}' no tiene sshKey configurada; endurecer SSH sin clave validada seria inseguro",
            target.name
        ))
    })?;

    let override_content = render_ssh_override(ssh_policy);
    let backup_path = format!("{}.bak", SSH_OVERRIDE_PATH);
    let cloud_init_backup_path = format!("{}.bak", CLOUD_INIT_SSH_PATH);
    let mut applied_steps = Vec::new();
    let mut warnings = Vec::new();

    applied_steps.push(format!("sshKey validada para manager: {ssh_key}"));
    if !ssh_policy.trusted_source_ips.is_empty() {
        warnings.push(format!(
            "TrustedSourceIps declaradas pero aun sin enforcement automatico: {}",
            ssh_policy.trusted_source_ips.join(", ")
        ));
    }

    if request.dry_run || !request.apply {
        applied_steps.push(format!(
            "Preview override SSH: {} bytes en {}.",
            override_content.len(),
            SSH_OVERRIDE_PATH
        ));
        return Ok(HardenSshReport {
            target: target.name.clone(),
            applied: false,
            reconnect_validated: false,
            override_path: SSH_OVERRIDE_PATH.to_string(),
            backup_path,
            applied_steps,
            warnings,
        });
    }

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let apply_script = format!(
        "set -e\nmkdir -p /etc/ssh/sshd_config.d\nif [ -f {override_path} ]; then cp -a {override_path} {backup_path}; else rm -f {backup_path}; fi\nif [ -f {cloud_init_path} ]; then cp -a {cloud_init_path} {cloud_init_backup_path}; printf %s {cloud_init_content} > {cloud_init_path}; chmod 644 {cloud_init_path}; fi\nprintf %s {content} > {override_path}\nchmod 600 {override_path}\nsshd -t\nservice_name=$(if systemctl status ssh >/dev/null 2>&1; then echo ssh; elif systemctl status sshd >/dev/null 2>&1; then echo sshd; else echo ssh; fi)\nsystemctl reload \"$service_name\"\necho SSH_HARDENED",
        override_path = shell_single_quote(SSH_OVERRIDE_PATH),
        backup_path = shell_single_quote(&backup_path),
        cloud_init_path = shell_single_quote(CLOUD_INIT_SSH_PATH),
        cloud_init_backup_path = shell_single_quote(&cloud_init_backup_path),
        cloud_init_content = shell_single_quote("PasswordAuthentication no\n"),
        content = shell_single_quote(&override_content),
    );
    let result = ssh.execute(&format!("bash -lc {}", shell_single_quote(&apply_script))).await?;
    if !result.success() || !result.stdout.contains("SSH_HARDENED") {
        return Err(CoolifyError::Validation(format!(
            "No se pudo aplicar endurecimiento SSH: {}{}",
            result.stdout, result.stderr
        )));
    }
    applied_steps.push("Override SSH escrito, cloud-init neutralizado y servicio recargado.".to_string());

    let mut validation_client = SshClient::from_vps(&VpsConfig {
        ip: target.vps.ip.clone(),
        user: target.vps.user.clone(),
        ssh_key: target.vps.ssh_key.clone(),
        ssh_password: None,
    });

    let reconnect_validated = validation_client.connect().await.is_ok();
    if !reconnect_validated {
        let rollback_script = format!(
            "set -e\nif [ -f {backup_path} ]; then mv -f {backup_path} {override_path}; else rm -f {override_path}; fi\nif [ -f {cloud_init_backup_path} ]; then mv -f {cloud_init_backup_path} {cloud_init_path}; fi\nservice_name=$(if systemctl status ssh >/dev/null 2>&1; then echo ssh; elif systemctl status sshd >/dev/null 2>&1; then echo sshd; else echo ssh; fi)\nsshd -t\nsystemctl reload \"$service_name\"\necho SSH_ROLLED_BACK",
            override_path = shell_single_quote(SSH_OVERRIDE_PATH),
            backup_path = shell_single_quote(&backup_path),
            cloud_init_path = shell_single_quote(CLOUD_INIT_SSH_PATH),
            cloud_init_backup_path = shell_single_quote(&cloud_init_backup_path),
        );
        let rollback_result = ssh
            .execute(&format!("bash -lc {}", shell_single_quote(&rollback_script)))
            .await?;
        if !rollback_result.success() || !rollback_result.stdout.contains("SSH_ROLLED_BACK") {
            return Err(CoolifyError::RolledBack(
                "La reconexion SSH fallo y el rollback tambien fallo; hace falta revisar el host manualmente.".to_string(),
            ));
        }
        return Err(CoolifyError::RolledBack(
            "La reconexion SSH con clave fallo tras endurecer; el override se revirtio automaticamente.".to_string(),
        ));
    }

    applied_steps.push("Reconexión SSH por clave validada tras recargar sshd.".to_string());
    Ok(HardenSshReport {
        target: target.name.clone(),
        applied: true,
        reconnect_validated,
        override_path: SSH_OVERRIDE_PATH.to_string(),
        backup_path: format!("{}, {}", backup_path, cloud_init_backup_path),
        applied_steps,
        warnings,
    })
}

fn render_ssh_override(policy: &SshSecurityPolicyConfig) -> String {
    let mut lines = vec![
        "# Managed by coolify-manager-rs".to_string(),
        "PubkeyAuthentication yes".to_string(),
        "KbdInteractiveAuthentication no".to_string(),
        "ChallengeResponseAuthentication no".to_string(),
    ];
    if policy.disable_password_auth {
        lines.push("PasswordAuthentication no".to_string());
    }
    if policy.allow_root_key_only {
        lines.push("PermitRootLogin prohibit-password".to_string());
    }
    lines.push(String::new());
    lines.join("\n")
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}