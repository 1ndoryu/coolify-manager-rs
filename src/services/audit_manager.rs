use crate::config::{DeploymentTargetConfig, Settings, VpsConfig};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AuditReport {
    pub target: String,
    pub load_average: String,
    pub cpu_contention_summary: String,
    pub pressure_summary: String,
    pub memory_summary: String,
    pub disk_summary: String,
    pub storage_benchmark_summary: String,
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
        maintenance_policy: None,
        security_policy: None,
        host_profile: None,
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
    let cpu_contention_summary = ssh
        .execute(
            r#"sh -lc 'for s in 1 2 3; do read cpu u1 n1 s1 i1 w1 irq1 si1 st1 _ < /proc/stat; sleep 1; read cpu u2 n2 s2 i2 w2 irq2 si2 st2 _ < /proc/stat; total1=$((u1+n1+s1+i1+w1+irq1+si1+st1)); total2=$((u2+n2+s2+i2+w2+irq2+si2+st2)); dt=$((total2-total1)); dbusy=$(((u2-u1)+(n2-n1)+(s2-s1)+(irq2-irq1)+(si2-si1))); diow=$((w2-w1)); dst=$((st2-st1)); awk -v sample=$s -v busy=$dbusy -v iow=$diow -v steal=$dst -v total=$dt "BEGIN { printf(\"sample%d busy=%.2f%% iowait=%.2f%% steal=%.2f%%; \", sample, (busy*100)/total, (iow*100)/total, (steal*100)/total); }"; done'"#,
        )
        .await?
        .stdout
        .trim()
        .to_string();
    let pressure_summary = ssh
        .execute(
            r#"sh -lc 'printf "cpu_some[%s] io_some[%s] io_full[%s]" "$(grep "^some" /proc/pressure/cpu 2>/dev/null | cut -d" " -f2-5)" "$(grep "^some" /proc/pressure/io 2>/dev/null | cut -d" " -f2-5)" "$(grep "^full" /proc/pressure/io 2>/dev/null | cut -d" " -f2-5)"'"#,
        )
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
    let storage_benchmark_summary = ssh
        .execute(
            r#"sh -lc 'fs=$(df -PT /var/tmp | awk "NR==2 {print $2}"); printf "fs=%s " "$fs"; for i in 1 2 3; do f=/var/tmp/cm-audit-dd-$i.bin; start=$(date +%s%N); dd if=/dev/zero of=$f bs=1M count=32 conv=fdatasync status=none; end=$(date +%s%N); printf "run%d_ms=%s " "$i" "$(((end-start)/1000000))"; rm -f $f; done'"#,
        )
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
    if cpu_contention_summary.contains("iowait=")
        && !cpu_contention_summary.contains("iowait=0.00%")
    {
        recommendations.push(
            "Se detectó iowait: revisar latencia de disco, vecinos del nodo o almacenamiento degradado".to_string(),
        );
    }
    if storage_benchmark_summary.contains("run1_ms=")
        && (storage_benchmark_summary.contains("run1_ms=2")
            || storage_benchmark_summary.contains("run2_ms=2")
            || storage_benchmark_summary.contains("run3_ms=2")
            || storage_benchmark_summary.contains("run1_ms=3")
            || storage_benchmark_summary.contains("run2_ms=3")
            || storage_benchmark_summary.contains("run3_ms=3")
            || storage_benchmark_summary.contains("run1_ms=4")
            || storage_benchmark_summary.contains("run2_ms=4")
            || storage_benchmark_summary.contains("run3_ms=4")
            || storage_benchmark_summary.contains("run1_ms=5")
            || storage_benchmark_summary.contains("run2_ms=5")
            || storage_benchmark_summary.contains("run3_ms=5")
            || storage_benchmark_summary.contains("run1_ms=6")
            || storage_benchmark_summary.contains("run2_ms=6")
            || storage_benchmark_summary.contains("run3_ms=6")
            || storage_benchmark_summary.contains("run1_ms=7")
            || storage_benchmark_summary.contains("run2_ms=7")
            || storage_benchmark_summary.contains("run3_ms=7")
            || storage_benchmark_summary.contains("run1_ms=8")
            || storage_benchmark_summary.contains("run2_ms=8")
            || storage_benchmark_summary.contains("run3_ms=8")
            || storage_benchmark_summary.contains("run1_ms=9")
            || storage_benchmark_summary.contains("run2_ms=9")
            || storage_benchmark_summary.contains("run3_ms=9"))
    {
        recommendations.push(
            "Escritura a disco lenta en /var/tmp: si se confirma en horarios distintos, conviene migrar o abrir incidencia al proveedor".to_string(),
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
        cpu_contention_summary,
        pressure_summary,
        memory_summary,
        disk_summary,
        storage_benchmark_summary,
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
