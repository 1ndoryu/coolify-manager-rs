use crate::config::{DeploymentTargetConfig, Settings, VpsConfig};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use serde::Serialize;
use std::collections::BTreeSet;

const CONTROL_PLANE_CONTAINERS: &[&str] = &[
    "coolify",
    "coolify-db",
    "coolify-redis",
    "coolify-realtime",
    "coolify-sentinel",
];
const COOLIFY_PROXY_CONTAINER: &str = "coolify-proxy";

#[derive(Debug, Clone, Serialize)]
pub struct ControlPlaneAuditReport {
    pub target: String,
    pub load_average: String,
    pub dominance_summary: String,
    pub container_summary: String,
    pub coolify_process_summary: String,
    pub supervisor_summary: String,
    pub scheduler_summary: String,
    pub horizon_summary: String,
    pub failed_job_summary: String,
    pub redis_summary: String,
    pub queue_summary: String,
    pub logs_summary: String,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone)]
struct ContainerStat {
    name: String,
    cpu_percent: f32,
    mem_usage: String,
    block_io: String,
}

#[derive(Debug, Clone)]
struct ProxyNetworkDrift {
    proxy_networks: Vec<String>,
    workload_networks: Vec<String>,
    missing_networks: Vec<String>,
}

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

pub async fn repair_default_vps(
    settings: &Settings,
) -> std::result::Result<Vec<String>, CoolifyError> {
    repair_vps_config("default", &settings.vps).await
}

pub async fn repair_target(
    target: &DeploymentTargetConfig,
) -> std::result::Result<Vec<String>, CoolifyError> {
    repair_vps_config(&target.name, &target.vps).await
}

pub async fn stop_control_plane_target(
    target: &DeploymentTargetConfig,
    include_proxy: bool,
) -> std::result::Result<Vec<String>, CoolifyError> {
    change_control_plane_state(&target.name, &target.vps, "stop", include_proxy).await
}

pub async fn start_control_plane_target(
    target: &DeploymentTargetConfig,
    include_proxy: bool,
) -> std::result::Result<Vec<String>, CoolifyError> {
    change_control_plane_state(&target.name, &target.vps, "start", include_proxy).await
}

pub async fn status_control_plane_target(
    target: &DeploymentTargetConfig,
    include_proxy: bool,
) -> std::result::Result<Vec<String>, CoolifyError> {
    change_control_plane_state(&target.name, &target.vps, "status", include_proxy).await
}

async fn audit_vps_config(
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

async fn repair_vps_config(
    target_name: &str,
    vps: &VpsConfig,
) -> std::result::Result<Vec<String>, CoolifyError> {
    let mut ssh = SshClient::from_vps(vps);
    ssh.connect().await?;

    let artisan_commands = exec_raw_script_timeout(
        &ssh,
        r#"docker exec coolify php artisan list --raw 2>/dev/null || true"#,
        60,
    )
    .await?;
    let supports_clear_metrics = artisan_commands
        .lines()
        .any(|line| line.trim() == "horizon:clear-metrics");
    let supports_schedule_clear_cache = artisan_commands
        .lines()
        .any(|line| line.trim() == "schedule:clear-cache");

    let failed_before = exec_raw_script_timeout(
        &ssh,
        r#"docker exec coolify sh -lc 'failed_lines=$(php artisan queue:failed 2>/dev/null | sed "/There are no failed jobs/d;/^[[:space:]]*$/d"); if [ -n "$failed_lines" ]; then echo "$failed_lines" | tail -n +2 | wc -l | tr -d " "; else echo 0; fi' 2>/dev/null || echo unknown"#,
        60,
    )
    .await?;

    let mut steps = vec![format!("target={target_name}")];
    steps.push(format!("failed_jobs_before={}", empty_as_unknown(&failed_before)));

    sync_proxy_network_drift(&ssh, &mut steps).await?;

    let queue_flush = exec_raw_script_timeout(
        &ssh,
        r#"docker exec coolify php artisan queue:flush --no-interaction 2>/dev/null || echo queue-flush-unavailable"#,
        120,
    )
    .await?;
    steps.push(format!("queue_flush={}", ok_if_empty(&queue_flush)));

    if supports_clear_metrics {
        let clear_metrics = exec_raw_script_timeout(
            &ssh,
            r#"docker exec coolify php artisan horizon:clear-metrics --no-interaction 2>/dev/null || echo horizon-clear-metrics-failed"#,
            120,
        )
        .await?;
        steps.push(format!("horizon_clear_metrics={}", ok_if_empty(&clear_metrics)));
    } else {
        steps.push("horizon_clear_metrics=unsupported".to_string());
    }

    if supports_schedule_clear_cache {
        let clear_schedule_cache = exec_raw_script_timeout(
            &ssh,
            r#"docker exec coolify php artisan schedule:clear-cache --no-interaction 2>/dev/null || echo schedule-clear-cache-failed"#,
            60,
        )
        .await?;
        steps.push(format!(
            "schedule_clear_cache={}",
            ok_if_empty(&clear_schedule_cache)
        ));
    }

    let terminate_horizon = exec_raw_script_timeout(
        &ssh,
        r#"docker exec coolify php artisan horizon:terminate --no-interaction 2>/dev/null || echo horizon-terminate-failed"#,
        60,
    )
    .await?;
    steps.push(format!("horizon_terminate={}", ok_if_empty(&terminate_horizon)));

    let failed_after = exec_raw_script_timeout(
        &ssh,
        r#"docker exec coolify sh -lc 'failed_lines=$(php artisan queue:failed 2>/dev/null | sed "/There are no failed jobs/d;/^[[:space:]]*$/d"); if [ -n "$failed_lines" ]; then echo "$failed_lines" | tail -n +2 | wc -l | tr -d " "; else echo 0; fi' 2>/dev/null || echo unknown"#,
        60,
    )
    .await?;
    steps.push(format!("failed_jobs_after={}", empty_as_unknown(&failed_after)));

    Ok(steps)
}

async fn load_horizon_summaries(
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

async fn change_control_plane_state(
    target_name: &str,
    vps: &VpsConfig,
    action: &str,
    include_proxy: bool,
) -> std::result::Result<Vec<String>, CoolifyError> {
    let mut ssh = SshClient::from_vps(vps);
    ssh.connect().await?;

    let containers = list_control_plane_containers(&ssh, include_proxy).await?;
    let running_before = list_running_control_plane_containers(&ssh, include_proxy).await?;
    let mut steps = vec![format!("target={target_name}")];
    steps.push(format!(
        "candidatos={}",
        if containers.is_empty() {
            "none".to_string()
        } else {
            containers.join(",")
        }
    ));
    steps.push(format!(
        "running_before={}",
        if running_before.is_empty() {
            "none".to_string()
        } else {
            running_before.join(",")
        }
    ));

    match action {
        "status" => {}
        "stop" => {
            let to_stop: Vec<String> = containers
                .iter()
                .filter(|name| running_before.iter().any(|running| running == *name))
                .cloned()
                .collect();
            if to_stop.is_empty() {
                steps.push("stop=no-running-control-plane-containers".to_string());
            } else {
                let stop_output = exec_raw_script_timeout(
                    &ssh,
                    &format!("docker stop {} 2>/dev/null || true", to_stop.join(" ")),
                    120,
                )
                .await?;
                steps.push(format!("stop={}", ok_if_empty(&stop_output)));
            }
        }
        "start" => {
            if containers.is_empty() {
                steps.push("start=no-control-plane-containers-found".to_string());
            } else {
                let start_output = exec_raw_script_timeout(
                    &ssh,
                    &format!("docker start {} 2>/dev/null || true", containers.join(" ")),
                    120,
                )
                .await?;
                steps.push(format!("start={}", ok_if_empty(&start_output)));
            }
        }
        _ => unreachable!("accion control-plane no soportada"),
    }

    let running_after = list_running_control_plane_containers(&ssh, include_proxy).await?;
    steps.push(format!(
        "running_after={}",
        if running_after.is_empty() {
            "none".to_string()
        } else {
            running_after.join(",")
        }
    ));

    Ok(steps)
}

async fn list_control_plane_containers(
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

async fn list_running_control_plane_containers(
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

async fn inspect_proxy_network_drift(
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

    let proxy_networks = parse_name_set(&proxy_networks).into_iter().collect::<Vec<_>>();
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

async fn sync_proxy_network_drift(
    ssh: &SshClient,
    steps: &mut Vec<String>,
) -> std::result::Result<(), CoolifyError> {
    let proxy_network_drift_before = inspect_proxy_network_drift(ssh).await?;
    steps.push(format!(
        "proxy_networks_before={}",
        format_name_list(&proxy_network_drift_before.proxy_networks)
    ));
    steps.push(format!(
        "workload_networks={}",
        format_name_list(&proxy_network_drift_before.workload_networks)
    ));
    steps.push(format!(
        "proxy_networks_missing_before={}",
        format_name_list(&proxy_network_drift_before.missing_networks)
    ));

    if proxy_network_drift_before.missing_networks.is_empty() {
        steps.push("proxy_network_sync=already-aligned".to_string());
    } else {
        for network in &proxy_network_drift_before.missing_networks {
            let connect_output = exec_raw_script_timeout(
                ssh,
                &format!(
                    "docker network connect {} {} 2>&1 || true",
                    sh_quote(network),
                    COOLIFY_PROXY_CONTAINER
                ),
                60,
            )
            .await?;
            steps.push(format!(
                "proxy_network_connect[{network}]={}",
                ok_if_empty(&connect_output)
            ));
        }
    }

    let proxy_network_drift_after = inspect_proxy_network_drift(ssh).await?;
    steps.push(format!(
        "proxy_networks_missing_after={}",
        format_name_list(&proxy_network_drift_after.missing_networks)
    ));

    Ok(())
}

fn is_control_plane_container(name: &str, include_proxy: bool) -> bool {
    CONTROL_PLANE_CONTAINERS.iter().any(|candidate| candidate == &name)
        || (include_proxy && name == COOLIFY_PROXY_CONTAINER)
}

fn parse_name_set(raw: &str) -> BTreeSet<String> {
    raw.lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| *value != "null")
        .map(ToString::to_string)
        .collect()
}

fn should_sync_proxy_network(network: &str) -> bool {
    !matches!(network, "bridge" | "host" | "none" | "ingress")
        && !network.ends_with("_ssh_net")
}

fn format_name_list(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(",")
    }
}

fn parse_container_stats(raw: &str) -> Vec<ContainerStat> {
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.split('|');
            let name = parts.next()?.trim();
            let cpu = parts.next()?.trim().trim_end_matches('%');
            let mem_usage = parts.next()?.trim();
            let block_io = parts.next()?.trim();
            Some(ContainerStat {
                name: name.to_string(),
                cpu_percent: cpu.parse::<f32>().unwrap_or(0.0),
                mem_usage: mem_usage.to_string(),
                block_io: block_io.to_string(),
            })
        })
        .collect()
}

fn build_dominance_summary(stats: &[ContainerStat]) -> String {
    let Some(first) = stats.first() else {
        return "sin-datos".to_string();
    };
    let second_cpu = stats.get(1).map(|stat| stat.cpu_percent).unwrap_or(0.0);
    let total_cpu: f32 = stats.iter().map(|stat| stat.cpu_percent).sum();
    let dominant = first.name == "coolify"
        && first.cpu_percent >= 50.0
        && (second_cpu == 0.0 || first.cpu_percent >= second_cpu * 3.0);

    if dominant {
        format!(
            "coolify domina el control-plane ({:.2}% CPU; siguiente {:.2}% CPU; total control-plane {:.2}% CPU)",
            first.cpu_percent, second_cpu, total_cpu
        )
    } else {
        format!(
            "sin dominancia extrema (hotspot={} {:.2}% CPU; total control-plane {:.2}% CPU)",
            first.name, first.cpu_percent, total_cpu
        )
    }
}

fn build_recommendations(
    stats: &[ContainerStat],
    load_average: &str,
    coolify_process_summary: &str,
    supervisor_summary: &str,
    scheduler_summary: &str,
    horizon_summary: &str,
    failed_job_summary: &str,
    redis_summary: &str,
    queue_summary: &str,
    logs_summary: &str,
) -> Vec<String> {
    let mut recommendations = Vec::new();
    let first = stats.first();
    let load_1m = load_average
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(0.0);

    if let Some(first) = first {
        if first.name == "coolify" && first.cpu_percent >= 50.0 {
            recommendations.push(
                "El hotspot principal es el contenedor coolify, no el sitio WordPress de prueba. Conviene revisar jobs/scheduler del panel antes de tocar el workload alojado.".to_string(),
            );
        }
    }

    if coolify_process_summary.contains("php") {
        recommendations.push(
            "Dentro del contenedor coolify la carga cae en procesos PHP; revisar scheduler, colas y tareas internas del panel.".to_string(),
        );
    }

    if supervisor_summary.contains("RUNNING") {
        recommendations.push(
            "Supervisor tiene procesos activos en el control-plane; si la CPU sigue alta, correlacionar el proceso PHP caliente con el servicio supervisado correspondiente.".to_string(),
        );
    }

    if scheduler_summary.contains("ScheduledJobManager")
        && scheduler_summary.contains("ServerManagerJob")
    {
        recommendations.push(
            "El scheduler de Coolify está activo cada minuto para `ScheduledJobManager` y `ServerManagerJob`; si uno se vuelve caro, la presión es sostenida incluso sin tráfico del sitio alojado.".to_string(),
        );
    }

    if horizon_summary.to_ascii_lowercase().contains("failed_jobs=")
        && !horizon_summary.contains("failed_jobs=0")
        && !horizon_summary.contains("failed_jobs=unknown")
    {
        recommendations.push(
            "Horizon o la cola tienen jobs fallidos pendientes; revisar esas fallas porque pueden reintentarse y sostener carga innecesaria.".to_string(),
        );
    }

    if failed_job_summary.contains("ConnectProxyToNetworksJob") {
        recommendations.push(
            "Los failed jobs recientes son `ConnectProxyToNetworksJob` con timeout. Eso apunta a costo/latencia al conectar redes del proxy desde el panel, no al WordPress alojado; conviene revisar attach de redes y llamadas SSH remotas de Coolify en este host.".to_string(),
        );
    }

    if queue_summary.contains("len=") && !queue_lengths_are_zero(queue_summary) {
        recommendations.push(
            "Redis muestra colas internas con backlog no trivial; el scheduler/Horizon puede estar gastando CPU simplemente en drenar o inspeccionar esa cola.".to_string(),
        );
    }

    if redis_summary.contains("blocked_clients=") && !redis_summary.contains("blocked_clients=0") {
        recommendations.push(
            "Redis tiene clientes bloqueados; eso refuerza que el cuello puede estar en colas/realtime mas que en el sitio alojado.".to_string(),
        );
    }

    if logs_summary.contains("ScheduledJobManager") {
        recommendations.push(
            "`ScheduledJobManager` aparece repetidamente en los logs del panel; en VPS2 conviene revisar si alguna tarea programada del control-plane se está volviendo lenta o se solapa.".to_string(),
        );
    }

    if logs_summary.contains("horizon:snapshot") {
        recommendations.push(
            "`horizon:snapshot` también aparece en el contenedor coolify. Si tarda varios segundos solo en VPS2, el panel puede estar más penalizado por I/O o por jobs internos que en VPS1.".to_string(),
        );
    }

    if logs_summary.contains("PushServerUpdateJob") {
        recommendations.push(
            "`PushServerUpdateJob` apareció en los logs del panel. Si solo se ve en VPS2 o tarda más allí, puede estar añadiendo trabajo extra del control-plane sobre ese host.".to_string(),
        );
    }

    if logs_summary.to_ascii_lowercase().contains("timeout")
        || logs_summary.to_ascii_lowercase().contains("failed")
        || logs_summary.to_ascii_lowercase().contains("exception")
    {
        recommendations.push(
            "Los logs recientes de coolify muestran errores o timeouts; revisar esas tareas porque pueden estar ciclando y sosteniendo CPU.".to_string(),
        );
    }

    if load_1m > 4.0 {
        recommendations.push(
            "Aunque ya haya swap, el control-plane sigue contribuyendo a un load por encima de los 4 vCPU. Separar Coolify del workload o mover sitios sensibles sigue siendo una opcion realista.".to_string(),
        );
    }

    if recommendations.is_empty() {
        recommendations.push(
            "El control-plane no aparece como hotspot dominante en esta toma; repetir el muestreo cuando el load vuelva a subir.".to_string(),
        );
    }

    recommendations
}

async fn exec_trim(ssh: &SshClient, command: &str) -> std::result::Result<String, CoolifyError> {
    let result = ssh.execute(command).await?;
    if !result.stdout.trim().is_empty() {
        return Ok(result.stdout.trim().replace('\n', " "));
    }
    Ok(result.stderr.trim().replace('\n', " "))
}

async fn exec_raw_script(
    ssh: &SshClient,
    script: &str,
) -> std::result::Result<String, CoolifyError> {
    exec_raw_script_timeout(ssh, script, 15).await
}

async fn exec_raw_script_timeout(
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

fn empty_as_unknown(value: &str) -> String {
    if value.trim().is_empty() {
        "unknown".to_string()
    } else {
        value.trim().to_string()
    }
}

fn empty_as_unknown_multiline(value: &str) -> String {
    if value.trim().is_empty() {
        "unknown".to_string()
    } else {
        value.trim().replace('\r', "")
    }
}

fn ok_if_empty(value: &str) -> String {
    if value.trim().is_empty() {
        "ok".to_string()
    } else {
        value.trim().replace('\n', " ").replace('\r', "")
    }
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn build_redis_cli_script(body: &str) -> String {
    format!(
        "bash -lc {}",
        sh_quote(&format!(
            "set -o pipefail\npassword=$(docker inspect coolify-redis --format '{{{{range .Config.Env}}}}{{{{println .}}}}{{{{end}}}}' 2>/dev/null | awk -F= '$1==\"REDIS_PASSWORD\" {{print substr($0, index($0, \"=\") + 1); exit}}')\nredis_cli() {{\n    if [ -n \"$password\" ]; then\n        docker exec coolify-redis redis-cli --no-auth-warning -a \"$password\" \"$@\"\n    else\n        docker exec coolify-redis redis-cli \"$@\"\n    fi\n}}\n{}",
            body
        ))
    )
}

fn queue_lengths_are_zero(summary: &str) -> bool {
    summary
        .split("len=")
        .skip(1)
        .all(|segment| segment.chars().next().is_some_and(|ch| ch == '0'))
}