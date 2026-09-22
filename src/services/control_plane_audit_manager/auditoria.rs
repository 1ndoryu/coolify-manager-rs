/* Barrido de auditoría del plano de control (119A-5 split control_plane_audit_manager). */

use super::ejecucion::{exec_raw_script, exec_raw_script_timeout, exec_trim};
use super::formato::{
    build_dominance_summary, empty_as_unknown, empty_as_unknown_multiline,
    is_control_plane_container, parse_container_stats, parse_name_set, sh_quote,
    should_sync_proxy_network,
};
use super::recomendaciones::build_recommendations;
use super::redis::build_redis_cli_script;
use super::tipos::{ControlPlaneAuditReport, ProxyNetworkDrift};
use crate::config::{DeploymentTargetConfig, Settings, VpsConfig};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use std::collections::BTreeSet;

/* [235A-2] Cuando el host muestra load alto, no basta con decir "es Coolify".
 * Este barrido separa el plano de control del workload alojado y detecta cuando
 * el control-plane se vuelve el hotspot dominante del nodo. */
pub async fn audit_default_vps(
    settings: &Settings,
    since: &str,
) -> std::result::Result<ControlPlaneAuditReport, CoolifyError> {
    audit_vps_config("default", &settings.vps, since).await
}

pub async fn audit_target(
    target: &DeploymentTargetConfig,
    since: &str,
) -> std::result::Result<ControlPlaneAuditReport, CoolifyError> {
    audit_vps_config(&target.name, &target.vps, since).await
}

pub(super) async fn audit_vps_config(
    target_name: &str,
    vps: &VpsConfig,
    since: &str,
) -> std::result::Result<ControlPlaneAuditReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(vps);
    ssh.connect().await?;

    let load_average = exec_trim(&ssh, "cat /proc/loadavg | awk '{print $1, $2, $3}'").await?;
    let stats_raw = exec_raw_script(
        &ssh,
        r#"if command -v docker >/dev/null 2>&1; then docker stats --no-stream --format '{{.Name}}|{{.CPUPerc}}|{{.MemUsage}}|{{.BlockIO}}' 2>/dev/null | grep '^coolify'; else echo docker-unavailable; fi"#,
    )
    .await?;
    let mut stats = parse_container_stats(&stats_raw);
    stats.sort_by(|left, right| {
        right
            .cpu_percent
            .partial_cmp(&left.cpu_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let dominance_summary = build_dominance_summary(&stats);
    let container_summary = stats
        .iter()
        .map(|stat| {
            format!(
                "{} cpu={:.2}% mem={} block={}",
                stat.name, stat.cpu_percent, stat.mem_usage, stat.block_io
            )
        })
        .collect::<Vec<_>>()
        .join("; ");

    let coolify_process_summary = exec_raw_script(
        &ssh,
        r#"docker exec coolify sh -lc 'ps -o pid,ppid,comm,args | grep -E "schedule:work|artisan horizon|horizon:supervisor|horizon:work|ssh -fNM" | grep -v grep | head -n 20 || echo unavailable' 2>/dev/null || echo unavailable"#,
    )
    .await?;
    let supervisor_summary = exec_raw_script(
        &ssh,
        r#"docker exec coolify sh -lc 'ps -o pid,ppid,comm,args | grep -E "s6-supervise scheduler-worker|s6-supervise horizon|s6-supervise php-fpm|s6-supervise nginx" | grep -v grep | head -n 12 || echo supervisor-unavailable' 2>/dev/null || echo supervisor-unavailable"#,
    )
    .await?;
    let scheduler_summary = exec_raw_script(
        &ssh,
        r#"docker exec coolify sh -lc 'php artisan schedule:list 2>/dev/null | sed -n "2,24p" | tr -s " " | sed "s/^ //" | grep -E "ScheduledJobManager|ServerManagerJob|horizon:snapshot|CleanupInstanceStuffsJob|uploads:clear|CheckForUpdatesJob|PullTemplatesFromCDN|PullChangelog" | tr "\n" ";" || echo schedule-unavailable'"#,
    )
    .await?;
    let (horizon_summary, failed_job_summary) = load_horizon_summaries(&ssh).await?;
    let redis_summary = exec_raw_script(
        &ssh,
        &build_redis_cli_script(
            r#"if redis_cli ping >/dev/null 2>&1; then redis_cli info 2>/dev/null | awk -F: '/^(used_memory_human|used_memory_peak_human|connected_clients|blocked_clients|instantaneous_ops_per_sec|total_commands_processed|keyspace_hits|keyspace_misses)$/ {gsub(/\r/, "", $2); printf "%s=%s; ", $1, $2; found=1} END {if (!found) print "redis-info-unavailable"}'; else echo redis-unavailable; fi"#,
        ),
    )
    .await?;
    let queue_summary = exec_raw_script(
        &ssh,
        &build_redis_cli_script(
            r#"if redis_cli ping >/dev/null 2>&1; then queue_keys=$(redis_cli --scan --pattern 'queues:*' 2>/dev/null | wc -l | tr -d ' '); horizon_keys=$(redis_cli --scan --pattern 'horizon:*' 2>/dev/null | wc -l | tr -d ' '); sample=$( (redis_cli --scan --pattern 'queues:*' 2>/dev/null; redis_cli --scan --pattern 'horizon:*' 2>/dev/null) | head -n 6 | tr '\n' ' ' | sed 's/  */ /g' | cut -c1-220); [ -n "$queue_keys" ] || queue_keys=0; [ -n "$horizon_keys" ] || horizon_keys=0; [ -n "$sample" ] || sample=none; printf "queue_keys=%s horizon_keys=%s sample=%s" "$queue_keys" "$horizon_keys" "$sample"; else echo redis-queues-unavailable; fi"#,
        ),
    )
    .await?;
    let log_script = format!(
        "docker logs --since {} coolify 2>&1 | grep -Ei 'error|exception|horizon|queue|schedule|backup|failed|timeout|poll' | tail -n 20 || docker logs --since {} coolify 2>&1 | tail -n 20 || true",
        sh_quote(since),
        sh_quote(since)
    );
    let logs_summary = exec_raw_script(&ssh, &log_script).await?;

    let recommendations = build_recommendations(
        &stats,
        &load_average,
        &coolify_process_summary,
        &supervisor_summary,
        &scheduler_summary,
        &horizon_summary,
        &failed_job_summary,
        &redis_summary,
        &queue_summary,
        &logs_summary,
    );

    Ok(ControlPlaneAuditReport {
        target: target_name.to_string(),
        load_average,
        dominance_summary,
        container_summary: empty_as_unknown(&container_summary),
        coolify_process_summary: empty_as_unknown_multiline(&coolify_process_summary),
        supervisor_summary: empty_as_unknown_multiline(&supervisor_summary),
        scheduler_summary: empty_as_unknown(&scheduler_summary),
        horizon_summary: empty_as_unknown(&horizon_summary),
        failed_job_summary: empty_as_unknown_multiline(&failed_job_summary),
        redis_summary: empty_as_unknown(&redis_summary),
        queue_summary: empty_as_unknown(&queue_summary),
        logs_summary: empty_as_unknown_multiline(&logs_summary),
        recommendations,
    })
}

pub(super) async fn load_horizon_summaries(
    ssh: &SshClient,
) -> std::result::Result<(String, String), CoolifyError> {
    let horizon_status = exec_raw_script(
        ssh,
        r#"docker exec coolify sh -lc 'php artisan horizon:status 2>/dev/null | grep -E "Horizon is|not running|inactive|paused" | head -n 1 | sed "s/^ *//" || echo horizon-unavailable'"#,
    )
    .await?;
    let horizon_failed_summary = exec_raw_script(
        ssh,
        r#"docker exec coolify sh -lc 'failed_lines=$(php artisan queue:failed 2>/dev/null | sed "/There are no failed jobs/d;/^[[:space:]]*$/d"); if [ -n "$failed_lines" ]; then failed=$(echo "$failed_lines" | tail -n +2 | wc -l | tr -d " "); sample=$(echo "$failed_lines" | tail -n +2 | head -n 3 | tr "\n" " " | sed "s/  */ /g" | cut -c1-220); else failed=0; sample=none; fi; [ -n "$failed" ] || failed=unknown; [ -n "$sample" ] || sample=none; printf "failed_jobs=%s sample=%s" "$failed" "$sample"' 2>/dev/null || echo failed_jobs=unknown sample=none"#,
    )
    .await?;
    let failed_job_summary = exec_raw_script(
        ssh,
        r#"docker exec coolify sh -lc 'php artisan tinker --execute="dump(DB::table(\"failed_jobs\")->orderByDesc(\"failed_at\")->limit(2)->get([\"failed_at\",\"uuid\",\"queue\",\"exception\"])->toArray());" 2>/dev/null | grep -E "failed_at|uuid|queue|TimeoutExceededException|ConnectProxyToNetworksJob" | head -n 16 | tr "\n" ";" || echo failed-jobs-unavailable'"#,
    )
    .await?;

    Ok((
        format!(
            "horizon={} {}",
            empty_as_unknown(&horizon_status),
            empty_as_unknown(&horizon_failed_summary)
        ),
        failed_job_summary,
    ))
}

pub(super) async fn list_control_plane_containers(
    ssh: &SshClient,
    include_proxy: bool,
) -> std::result::Result<Vec<String>, CoolifyError> {
    let raw = exec_raw_script_timeout(
        ssh,
        r#"docker ps -a --format '{{.Names}}' 2>/dev/null || true"#,
        30,
    )
    .await?;
    Ok(raw
        .lines()
        .map(str::trim)
        .filter(|name| is_control_plane_container(name, include_proxy))
        .map(ToString::to_string)
        .collect())
}

pub(super) async fn list_running_control_plane_containers(
    ssh: &SshClient,
    include_proxy: bool,
) -> std::result::Result<Vec<String>, CoolifyError> {
    let raw = exec_raw_script_timeout(
        ssh,
        r#"docker ps --format '{{.Names}}' 2>/dev/null || true"#,
        30,
    )
    .await?;
    Ok(raw
        .lines()
        .map(str::trim)
        .filter(|name| is_control_plane_container(name, include_proxy))
        .map(ToString::to_string)
        .collect())
}

pub(super) async fn inspect_proxy_network_drift(
    ssh: &SshClient,
) -> std::result::Result<ProxyNetworkDrift, CoolifyError> {
    let proxy_networks = exec_raw_script_timeout(
        ssh,
        r#"docker inspect coolify-proxy --format '{{range $k,$v := .NetworkSettings.Networks}}{{println $k}}{{end}}' 2>/dev/null || true"#,
        30,
    )
    .await?;
    let workload_networks = exec_raw_script_timeout(
        ssh,
        r#"docker ps --format '{{.Names}}' 2>/dev/null | while read name; do case "$name" in coolify|coolify-db|coolify-redis|coolify-realtime|coolify-sentinel|coolify-proxy|ssh-*) continue ;; esac; docker inspect "$name" --format '{{range $k,$v := .NetworkSettings.Networks}}{{println $k}}{{end}}' 2>/dev/null; done || true"#,
        60,
    )
    .await?;

    let proxy_networks = parse_name_set(&proxy_networks)
        .into_iter()
        .collect::<Vec<_>>();
    let workload_networks = parse_name_set(&workload_networks)
        .into_iter()
        .filter(|network| should_sync_proxy_network(network))
        .collect::<Vec<_>>();
    let proxy_set = proxy_networks.iter().cloned().collect::<BTreeSet<_>>();
    let workload_set = workload_networks.iter().cloned().collect::<BTreeSet<_>>();
    let missing_networks = workload_set
        .difference(&proxy_set)
        .cloned()
        .collect::<Vec<_>>();

    Ok(ProxyNetworkDrift {
        proxy_networks,
        workload_networks,
        missing_networks,
    })
}
