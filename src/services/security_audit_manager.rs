use crate::config::{DeploymentTargetConfig, Settings, VpsConfig};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SecurityAuditReport {
    pub target: String,
    pub sshd_summary: String,
    pub ssh_activity_summary: String,
    pub firewall_summary: String,
    pub fail2ban_summary: String,
    pub docker_ports_summary: String,
    pub user_summary: String,
    pub recommendations: Vec<String>,
}

pub async fn audit_default_vps(
    settings: &Settings,
) -> std::result::Result<SecurityAuditReport, CoolifyError> {
    audit_vps_config("default", &settings.vps, None).await
}

pub async fn audit_target(
    target: &DeploymentTargetConfig,
) -> std::result::Result<SecurityAuditReport, CoolifyError> {
    audit_vps_config(&target.name, &target.vps, target.security_policy.as_ref()).await
}

async fn audit_vps_config(
    target_name: &str,
    vps: &VpsConfig,
    policy: Option<&crate::config::SecurityPolicyConfig>,
) -> std::result::Result<SecurityAuditReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(vps);
    ssh.connect().await?;

    let sshd_summary = exec_trim(
        &ssh,
        "bash -lc 'if command -v sshd >/dev/null 2>&1; then sshd -T 2>/dev/null | egrep \"^(permitrootlogin|passwordauthentication|pubkeyauthentication|maxauthtries|maxsessions) \" | tr \"\n\" \";\"; else echo sshd-unavailable; fi'",
    )
    .await?;
    let ssh_activity_summary = exec_trim(
        &ssh,
        "bash -lc 'active=$(who 2>/dev/null | head -n 10 | tr \"\n\" \";\" || true); recent=$(grep -h \"Accepted \" /var/log/auth.log /var/log/auth.log.1 2>/dev/null | tail -n 5 | tr \"\n\" \";\" || true); printf \"active=%s recent=%s\" \"${active:-none}\" \"${recent:-none}\"'",
    )
    .await?;
    let firewall_summary = exec_trim(
        &ssh,
        "bash -lc 'if command -v ufw >/dev/null 2>&1; then (ufw status verbose 2>/dev/null || ufw status 2>/dev/null) | head -n 20 | tr \"\n\" \";\"; elif command -v nft >/dev/null 2>&1; then nft list ruleset 2>/dev/null | head -n 40 | tr \"\n\" \";\"; else echo firewall-unavailable; fi'",
    )
    .await?;
    let fail2ban_summary = exec_trim(
        &ssh,
        "bash -lc 'state=$(systemctl is-active fail2ban 2>/dev/null || echo inactive); version=$(fail2ban-client --version 2>/dev/null | head -n 1 || true); printf \"state=%s version=%s\" \"$state\" \"${version:-unknown}\"'",
    )
    .await?;
    let docker_ports_summary = exec_trim(
        &ssh,
        "bash -lc 'if command -v docker >/dev/null 2>&1; then docker ps --format \"{{.Names}}|{{.Ports}}\" 2>/dev/null | tr \"\n\" \";\"; else echo docker-unavailable; fi'",
    )
    .await?;
    let user_summary = exec_trim(
        &ssh,
        "bash -lc 'getent passwd | awk -F: \"$3 >= 1000 && $1 != \\\"nobody\\\" {{print $1 \\\" uid=\\\" $3}}\" | tr \"\n\" \";\"'",
    )
    .await?;

    let recommendations = build_recommendations(
        policy,
        &sshd_summary,
        &firewall_summary,
        &fail2ban_summary,
        &docker_ports_summary,
    );

    Ok(SecurityAuditReport {
        target: target_name.to_string(),
        sshd_summary: empty_as_unknown(&sshd_summary),
        ssh_activity_summary: empty_as_unknown(&ssh_activity_summary),
        firewall_summary: empty_as_unknown(&firewall_summary),
        fail2ban_summary: empty_as_unknown(&fail2ban_summary),
        docker_ports_summary: empty_as_unknown(&docker_ports_summary),
        user_summary: empty_as_unknown(&user_summary),
        recommendations,
    })
}

fn build_recommendations(
    policy: Option<&crate::config::SecurityPolicyConfig>,
    sshd_summary: &str,
    firewall_summary: &str,
    fail2ban_summary: &str,
    docker_ports_summary: &str,
) -> Vec<String> {
    let mut notes = Vec::new();

    if sshd_summary.contains("passwordauthentication yes") {
        notes.push("SSH permite password auth; conviene desactivarlo o al menos restringirlo por politica declarada.".to_string());
    }
    if sshd_summary.contains("permitrootlogin yes") {
        notes.push("SSH permite root login sin endurecimiento adicional; revisa si debe quedar solo con clave o detras de IPs confiables.".to_string());
    }
    if firewall_summary.to_ascii_lowercase().contains("inactive") {
        notes.push("Firewall del host inactivo; si Traefik/Coolify no es la unica barrera, hay superficie expuesta innecesariamente.".to_string());
    }
    if fail2ban_summary.contains("state=inactive") {
        notes.push("fail2ban no esta activo; al menos la auditoria debe dejarlo visible cuando SSH quede abierto al mundo.".to_string());
    }
    if docker_ports_summary.contains("0.0.0.0:") {
        notes.push("Hay contenedores publicando puertos en 0.0.0.0; revisa si todos deben ser realmente publicos o quedar solo detras de proxy/red privada.".to_string());
    }

    if let Some(policy) = policy {
        if let Some(ssh) = &policy.ssh {
            if ssh.disable_password_auth && sshd_summary.contains("passwordauthentication yes") {
                notes.push("La politica declarada exige disablePasswordAuth, pero el host aun expone passwordauthentication yes.".to_string());
            }
            if ssh.allow_root_key_only
                && !sshd_summary.contains("permitrootlogin prohibit-password")
                && !sshd_summary.contains("permitrootlogin forced-commands-only")
                && !sshd_summary.contains("permitrootlogin without-password")
            {
                notes.push("La politica declarada pide root solo con clave, pero sshd no refleja una modalidad restringida de PermitRootLogin.".to_string());
            }
            if !ssh.trusted_source_ips.is_empty()
                && ssh
                    .trusted_source_ips
                    .iter()
                    .any(|ip| !firewall_summary.contains(ip))
            {
                notes.push(format!(
                    "Trusted IPs declaradas: {}. El firewall del host no refleja todavia toda esa lista.",
                    ssh.trusted_source_ips.join(", ")
                ));
            }
        }
        if let Some(firewall) = &policy.firewall {
            if firewall.enabled && firewall_summary.to_ascii_lowercase().contains("inactive") {
                notes.push("La politica declarada exige firewall activo, pero la auditoria detecta host sin firewall operativo.".to_string());
            }
            if !firewall.allowed_tcp_ports.is_empty()
                && firewall
                    .allowed_tcp_ports
                    .iter()
                    .any(|port| !firewall_summary.contains(&format!("{port}/tcp")))
            {
                notes.push(format!(
                    "Puertos TCP permitidos por politica: {}. El firewall del host no refleja todavia todos esos puertos.",
                    firewall.allowed_tcp_ports
                        .iter()
                        .map(u16::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }

    if notes.is_empty() {
        notes.push("No se detectaron desviaciones obvias en la auditoria corta; aun falta hardening activo y enforcement declarativo.".to_string());
    }

    notes
}

async fn exec_trim(ssh: &SshClient, command: &str) -> std::result::Result<String, CoolifyError> {
    let result = ssh.execute(command).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo ejecutando comando remoto: {}{}",
            result.stdout, result.stderr
        )));
    }
    Ok(result.stdout.trim().to_string())
}

fn empty_as_unknown(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}