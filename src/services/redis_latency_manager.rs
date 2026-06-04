use crate::config::{DeploymentTargetConfig, Settings, VpsConfig};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct RedisLatencyAuditReport {
    pub target: String,
    pub redis_ping: String,
    pub thp_summary: String,
    pub sysctl_summary: String,
    pub latency_summary: String,
    pub slowlog_summary: String,
    pub redis_info_summary: String,
    pub recommendations: Vec<String>,
}

pub async fn audit_default_vps(
    settings: &Settings,
    slowlog_count: u16,
) -> std::result::Result<RedisLatencyAuditReport, CoolifyError> {
    audit_vps_config("default", &settings.vps, slowlog_count).await
}

pub async fn audit_target(
    target: &DeploymentTargetConfig,
    slowlog_count: u16,
) -> std::result::Result<RedisLatencyAuditReport, CoolifyError> {
    audit_vps_config(&target.name, &target.vps, slowlog_count).await
}

async fn audit_vps_config(
    target_name: &str,
    vps: &VpsConfig,
    slowlog_count: u16,
) -> std::result::Result<RedisLatencyAuditReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(vps);
    ssh.connect().await?;

    let redis_ping = exec_trim(
        &ssh,
        &build_redis_cli_script("redis_cli ping 2>/dev/null || echo redis-unavailable"),
    )
    .await?;
    let thp_summary = exec_trim(
        &ssh,
        "bash -lc 'printf \"enabled=%s defrag=%s\" \"$(cat /sys/kernel/mm/transparent_hugepage/enabled 2>/dev/null | tr \"\\n\" \" \" | sed \"s/  */ /g\")\" \"$(cat /sys/kernel/mm/transparent_hugepage/defrag 2>/dev/null | tr \"\\n\" \" \" | sed \"s/  */ /g\")\"'",
    )
    .await?;
    let sysctl_summary = exec_trim(
        &ssh,
        "bash -lc 'printf \"vm.overcommit_memory=%s vm.swappiness=%s\" \"$(sysctl -n vm.overcommit_memory 2>/dev/null || echo unknown)\" \"$(sysctl -n vm.swappiness 2>/dev/null || echo unknown)\"'",
    )
    .await?;
    let latency_summary = exec_trim(
        &ssh,
        &build_redis_cli_script(
            "redis_cli latency latest 2>/dev/null | tr \"\\n\" \";\" || echo latency-unavailable",
        ),
    )
    .await?;
    let slowlog_summary = exec_trim(
        &ssh,
        &build_redis_cli_script(&format!(
            "redis_cli slowlog get {} 2>/dev/null | tr \"\\n\" \";\" || echo slowlog-unavailable",
            slowlog_count.max(1)
        )),
    )
    .await?;
    let redis_info_summary = exec_trim(
        &ssh,
        &build_redis_cli_script("redis_cli info 2>/dev/null | awk -F: '/^(used_memory_human|used_memory_peak_human|mem_fragmentation_ratio|blocked_clients|connected_clients|latest_fork_usec|instantaneous_ops_per_sec|aof_enabled|rdb_last_bgsave_status)$/ {gsub(/\\r/, \"\", $2); printf \"%s=%s; \", $1, $2; found=1} END {if (!found) print \"info-unavailable\"}'"),
    )
    .await?;

    let recommendations = build_recommendations(
        &thp_summary,
        &sysctl_summary,
        &latency_summary,
        &slowlog_summary,
    );

    Ok(RedisLatencyAuditReport {
        target: target_name.to_string(),
        redis_ping: empty_as_unknown(&redis_ping),
        thp_summary: empty_as_unknown(&thp_summary),
        sysctl_summary: empty_as_unknown(&sysctl_summary),
        latency_summary: empty_as_unknown(&latency_summary),
        slowlog_summary: empty_as_unknown(&slowlog_summary),
        redis_info_summary: empty_as_unknown(&redis_info_summary),
        recommendations,
    })
}

fn build_recommendations(
    thp_summary: &str,
    sysctl_summary: &str,
    latency_summary: &str,
    slowlog_summary: &str,
) -> Vec<String> {
    let mut notes = Vec::new();
    if !thp_summary.contains("[never]") && !thp_summary.contains("enabled=never") {
        notes.push("THP sigue activo; conviene dejarlo en never para reducir latencias y pausas de Redis en este nodo.".to_string());
    }
    if !sysctl_summary.contains("vm.overcommit_memory=1") {
        notes.push("vm.overcommit_memory no esta en 1; Redis puede sufrir warning/forks mas caros bajo presion.".to_string());
    }
    if latency_summary != "latency-unavailable"
        && !latency_summary.trim().is_empty()
        && latency_summary != "unknown"
    {
        notes.push("Redis reporta eventos de latencia; compara estos eventos con los picos del control-plane y del host.".to_string());
    }
    if slowlog_summary != "slowlog-unavailable"
        && !slowlog_summary.contains("(empty array)")
        && !slowlog_summary.contains("unknown")
    {
        notes.push("SLOWLOG contiene entradas; hay trabajo Redis lo bastante lento como para revisar comandos o presion del nodo.".to_string());
    }
    if notes.is_empty() {
        notes.push("No aparecieron señales obvias de Redis fuera del baseline corto; el ruido puede venir mas del panel o del host que de Redis puro.".to_string());
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

fn build_redis_cli_script(body: &str) -> String {
    let script = format!(
        "set -o pipefail\npassword=$(docker inspect coolify-redis --format '{{{{range .Config.Env}}}}{{{{println .}}}}{{{{end}}}}' 2>/dev/null | awk -F= '$1==\"REDIS_PASSWORD\" {{print substr($0, index($0, \"=\") + 1); exit}}')\nredis_cli() {{\n    if [ -n \"$password\" ]; then\n        docker exec coolify-redis redis-cli --no-auth-warning -a \"$password\" \"$@\"\n    else\n        docker exec coolify-redis redis-cli \"$@\"\n    fi\n}}\n{}",
        body
    );
    format!("bash -lc {}", sh_quote(&script))
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
