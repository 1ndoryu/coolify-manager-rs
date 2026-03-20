use crate::config::{DeploymentTargetConfig, Settings, VpsConfig};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AuditReport {
    pub target: String,
    pub load_average: String,
    pub memory_summary: String,
    pub disk_summary: String,
    pub docker_summary: String,
    pub security_summary: String,
    pub recommendations: Vec<String>,
}

pub async fn audit_default_vps(
    settings: &Settings,
) -> std::result::Result<AuditReport, CoolifyError> {
    let target = DeploymentTargetConfig {
        name: "default".to_string(),
        vps: settings.vps.clone(),
        coolify: settings.coolify.clone(),
    };
    audit_target(&target).await
}

pub async fn audit_target(
    target: &DeploymentTargetConfig,
) -> std::result::Result<AuditReport, CoolifyError> {
    let mut ssh = build_ssh(&target.vps);
    ssh.connect().await?;
    collect_audit(&ssh, &target.name).await
}

pub async fn audit_vps_config(
    name: &str,
    vps: &VpsConfig,
) -> std::result::Result<AuditReport, CoolifyError> {
    let mut ssh = build_ssh(vps);
    ssh.connect().await?;
    collect_audit(&ssh, name).await
}

fn build_ssh(vps: &VpsConfig) -> SshClient {
    SshClient::from_vps(vps)
}

async fn collect_audit(
    ssh: &SshClient,
    target: &str,
) -> std::result::Result<AuditReport, CoolifyError> {
    let load_average = ssh
        .execute("cat /proc/loadavg | awk '{print $1, $2, $3}'")
        .await?
        .stdout
        .trim()
        .to_string();
    let memory_summary = ssh
        .execute("free -m | awk 'NR==2 {printf \"used=%sMB free=%sMB total=%sMB\", $3, $4, $2}'")
        .await?
        .stdout
        .trim()
        .to_string();
    let disk_summary = ssh
        .execute("df -h / | awk 'NR==2 {printf \"used=%s available=%s use=%s\", $3, $4, $5}'")
        .await?
        .stdout
        .trim()
        .to_string();
    let docker_summary = ssh
        .execute("docker ps --format '{{.Names}}={{.Status}}' | head -20")
        .await?
        .stdout
        .trim()
        .to_string();
    let ufw_status = ssh
        .execute("(ufw status 2>/dev/null || true) | head -5")
        .await?
        .stdout
        .trim()
        .to_string();
    let fail2ban_status = ssh
        .execute("(systemctl is-active fail2ban 2>/dev/null || true)")
        .await?
        .stdout
        .trim()
        .to_string();
    let security_summary = format!(
        "ufw=[{}] fail2ban=[{}]",
        empty_as_unknown(&ufw_status),
        empty_as_unknown(&fail2ban_status)
    );

    let mut recommendations = Vec::new();
    if load_average
        .split_whitespace()
        .next()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(0.0)
        > 4.0
    {
        recommendations.push(
            "Carga alta: revisar procesos PHP/Node y concurrencia en contenedores".to_string(),
        );
    }
    if disk_summary.contains("9") && disk_summary.contains('%') {
        recommendations.push(
            "Disco con uso alto: purgar logs, imágenes Docker y backups huérfanos".to_string(),
        );
    }
    if !security_summary.contains("active") {
        recommendations.push(
            "Revisar firewall/fail2ban; no se detectó protección activa completa".to_string(),
        );
    }

    Ok(AuditReport {
        target: target.to_string(),
        load_average,
        memory_summary,
        disk_summary,
        docker_summary,
        security_summary,
        recommendations,
    })
}

fn empty_as_unknown(value: &str) -> &str {
    if value.is_empty() {
        "unknown"
    } else {
        value
    }
}
