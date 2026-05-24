use crate::config::{DeploymentTargetConfig, Settings, VpsConfig};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use serde::Serialize;

const SYSCTL_CONFIG_PATH: &str = "/etc/sysctl.d/99-coolify-manager-host.conf";
const SWAPFILE_PATH: &str = "/swapfile";
const THP_SERVICE_PATH: &str = "/etc/systemd/system/cm-disable-thp.service";
const DOCKER_DAEMON_CONFIG_PATH: &str = "/etc/docker/daemon.json";

/* [235A-1] Las optimizaciones host-level deben pasar por coolify-manager-rs para que
 * swap, sysctl y diagnostico queden repetibles y auditables sin depender de SSH manual. */
#[derive(Debug, Clone)]
pub struct HostOptimizationRequest {
    pub swap_gb: u16,
    pub swappiness: u8,
    pub vfs_cache_pressure: u16,
    pub overcommit_memory: u8,
    pub disable_thp: bool,
    pub docker_live_restore: bool,
    pub dry_run: bool,
    pub samples: u8,
    pub interval_seconds: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostOptimizationReport {
    pub target: String,
    pub os_name: String,
    pub load_average: String,
    pub pressure_summary: String,
    pub memory_summary: String,
    pub sampling_summary: String,
    pub ssh_sessions_summary: String,
    pub ssh_recent_summary: String,
    pub swap_before: String,
    pub swap_after: String,
    pub sysctl_summary: String,
    pub thp_summary: String,
    pub docker_runtime_summary: String,
    pub top_processes: String,
    pub docker_stats: String,
    pub applied_steps: Vec<String>,
    pub recommendations: Vec<String>,
}

struct HostSnapshot {
    os_name: String,
    load_average: String,
    pressure_summary: String,
    memory_summary: String,
    sampling_summary: String,
    ssh_sessions_summary: String,
    ssh_recent_summary: String,
    swap_summary: String,
    sysctl_summary: String,
    thp_summary: String,
    docker_runtime_summary: String,
    top_processes: String,
    docker_stats: String,
    cpu_count: usize,
}

pub async fn optimize_default_vps(
    settings: &Settings,
    request: &HostOptimizationRequest,
) -> std::result::Result<HostOptimizationReport, CoolifyError> {
    optimize_vps_config("default", &settings.vps, request).await
}

pub async fn optimize_target(
    target: &DeploymentTargetConfig,
    request: &HostOptimizationRequest,
) -> std::result::Result<HostOptimizationReport, CoolifyError> {
    optimize_vps_config(&target.name, &target.vps, request).await
}

pub async fn optimize_vps_config(
    target_name: &str,
    vps: &VpsConfig,
    request: &HostOptimizationRequest,
) -> std::result::Result<HostOptimizationReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(vps);
    ssh.connect().await?;

    let before = collect_snapshot(&ssh, 1, request.interval_seconds).await?;
    let mut applied_steps = Vec::new();

    if !swap_is_active(&before.swap_summary) {
        if request.dry_run {
            applied_steps.push(format!(
                "Dry run: se crearia {SWAPFILE_PATH} de {}G y se registraria en /etc/fstab.",
                request.swap_gb
            ));
        } else {
            let created = ensure_swap(&ssh, request.swap_gb).await?;
            applied_steps.push(created);
        }
    } else {
        applied_steps.push("Swap ya activa; no se modifico el host.".to_string());
    }

    let current_sysctl = parse_sysctl_values(&before.sysctl_summary);
    if current_sysctl
        != (
            request.swappiness,
            request.vfs_cache_pressure,
            request.overcommit_memory,
        )
    {
        if request.dry_run {
            applied_steps.push(format!(
                "Dry run: se escribiria {} con vm.swappiness={}, vm.vfs_cache_pressure={} y vm.overcommit_memory={}",
                SYSCTL_CONFIG_PATH, request.swappiness, request.vfs_cache_pressure, request.overcommit_memory
            ));
        } else {
            let updated = ensure_sysctl_profile(
                &ssh,
                request.swappiness,
                request.vfs_cache_pressure,
                request.overcommit_memory,
            )
            .await?;
            applied_steps.push(updated);
        }
    } else {
        applied_steps.push("Sysctl ya estaba alineado; no se modifico vm.swappiness/vm.vfs_cache_pressure/vm.overcommit_memory.".to_string());
    }

    if request.disable_thp {
        if thp_is_disabled(&before.thp_summary) {
            applied_steps.push("THP ya estaba desactivado; no se modifico el host.".to_string());
        } else if request.dry_run {
            applied_steps.push(format!(
                "Dry run: se desactivaria THP en runtime y se instalaria {} para persistirlo.",
                THP_SERVICE_PATH
            ));
        } else {
            applied_steps.push(ensure_thp_disabled(&ssh).await?);
        }
    }

    if request.docker_live_restore {
        if docker_live_restore_enabled(&before.docker_runtime_summary) {
            applied_steps.push("Docker ya reporta live-restore habilitado; no se toco daemon.json.".to_string());
        } else if request.dry_run {
            applied_steps.push(format!(
                "Dry run: se persistiria live-restore=true en {} y se intentaria recargar Docker sin reinicio agresivo.",
                DOCKER_DAEMON_CONFIG_PATH
            ));
        } else {
            applied_steps.push(ensure_docker_live_restore(&ssh).await?);
        }
    }

    let after = collect_snapshot(&ssh, request.samples, request.interval_seconds).await?;
    let recommendations = build_recommendations(&before, &after, request);

    Ok(HostOptimizationReport {
        target: target_name.to_string(),
        os_name: before.os_name,
        load_average: after.load_average,
        pressure_summary: after.pressure_summary,
        memory_summary: after.memory_summary,
        sampling_summary: after.sampling_summary,
        ssh_sessions_summary: after.ssh_sessions_summary,
        ssh_recent_summary: after.ssh_recent_summary,
        swap_before: before.swap_summary,
        swap_after: after.swap_summary,
        sysctl_summary: after.sysctl_summary,
        thp_summary: after.thp_summary,
        docker_runtime_summary: after.docker_runtime_summary,
        top_processes: after.top_processes,
        docker_stats: after.docker_stats,
        applied_steps,
        recommendations,
    })
}

async fn collect_snapshot(
    ssh: &SshClient,
    samples: u8,
    interval_seconds: u8,
) -> std::result::Result<HostSnapshot, CoolifyError> {
    let os_name = exec_trim(
        ssh,
        "sh -lc '. /etc/os-release 2>/dev/null; echo ${PRETTY_NAME:-unknown}'",
    )
    .await?;
    let cpu_count = exec_trim(ssh, "nproc 2>/dev/null || echo 1")
        .await?
        .parse::<usize>()
        .unwrap_or(1);
    let load_average = exec_trim(ssh, "cat /proc/loadavg | awk '{print $1, $2, $3}'").await?;
    let pressure_summary = exec_script(
        ssh,
        r#"printf "cpu_some[%s] io_some[%s] io_full[%s]" "$(grep "^some" /proc/pressure/cpu 2>/dev/null | cut -d" " -f2-5)" "$(grep "^some" /proc/pressure/io 2>/dev/null | cut -d" " -f2-5)" "$(grep "^full" /proc/pressure/io 2>/dev/null | cut -d" " -f2-5)""#,
    )
    .await?;
    let memory_summary = exec_trim(
        ssh,
        "free -m | awk 'NR==2 {printf \"used=%sMB free=%sMB total=%sMB\", $3, $4, $2}'",
    )
    .await?;
    let sampling_summary = build_sampling_summary(samples, interval_seconds);
    let ssh_sessions_summary = exec_script(
        ssh,
        r#"active=$(ss -H -tn state established '( sport = :22 )' 2>/dev/null | awk '{peer=$4; sub(/:[^:]*$/, "", peer); gsub(/^\[/, "", peer); gsub(/\]$/, "", peer); if (peer != "") counts[peer]++} END {for (peer in counts) printf "%s active=%d; ", peer, counts[peer]}'); who_hosts=$(who 2>/dev/null | awk '{host=$5; gsub(/[()]/, "", host); if (host=="") host="local"; counts[host]++} END {for (host in counts) printf "%s who=%d; ", host, counts[host]}'); if [ -n "$active" ] || [ -n "$who_hosts" ]; then printf "%s%s" "$active" "$who_hosts"; else echo none; fi"#,
    )
    .await?;
    let ssh_recent_summary = exec_script_timeout(
        ssh,
        r#"(journalctl -u ssh --since '-12 hours' --no-pager 2>/dev/null || grep 'Accepted' /var/log/auth.log 2>/dev/null || true) | awk '/Accepted/ {for (i=1; i<=NF; i++) if ($i=="from") { counts[$(i+1)]++ }} END {for (ip in counts) printf "%s %d\n", ip, counts[ip]; if (length(counts)==0) printf "none 0\n"}' | sort -k2 -nr | head -5 | awk '{ if ($1=="none") { printf "none" } else { printf "%s accepted=%s; ", $1, $2 } }'"#,
        20,
    )
    .await?;
    let swap_summary = exec_script(
        ssh,
        r#"out=$(swapon --show --noheadings --output NAME,TYPE,SIZE,USED,PRIO 2>/dev/null || true); if [ -n "$out" ]; then echo "$out" | awk '{printf "%s type=%s size=%s used=%s prio=%s; ", $1, $2, $3, $4, $5}'; else echo inactive; fi"#,
    )
    .await?;
    let sysctl_summary = exec_trim(
        ssh,
        r#"sh -lc 'printf "vm.swappiness=%s vm.vfs_cache_pressure=%s vm.overcommit_memory=%s" "$(sysctl -n vm.swappiness 2>/dev/null || echo unknown)" "$(sysctl -n vm.vfs_cache_pressure 2>/dev/null || echo unknown)" "$(sysctl -n vm.overcommit_memory 2>/dev/null || echo unknown)"'"#,
    )
    .await?;
    let thp_summary = exec_script(
        ssh,
        r#"printf "enabled=%s defrag=%s" "$(cat /sys/kernel/mm/transparent_hugepage/enabled 2>/dev/null | tr '\n' ' ' | sed 's/  */ /g')" "$(cat /sys/kernel/mm/transparent_hugepage/defrag 2>/dev/null | tr '\n' ' ' | sed 's/  */ /g')""#,
    )
    .await?;
    let docker_runtime_summary = exec_script_timeout(
        ssh,
        r#"if ! command -v docker >/dev/null 2>&1; then echo docker-unavailable; exit 0; fi; runtime=$(docker info 2>/dev/null | awk -F: '/Live Restore Enabled/ {gsub(/^ +/, "", $2); print $2}' | head -n 1); config=$(python3 -c "import json, pathlib; p=pathlib.Path('/etc/docker/daemon.json'); data=json.loads(p.read_text()) if p.exists() and p.stat().st_size else {}; print(str(data.get('live-restore', 'missing')).lower())" 2>/dev/null || echo unknown); printf "live_restore_runtime=%s live_restore_config=%s" "${runtime:-unknown}" "${config:-unknown}""#,
        20,
    )
    .await?;
    let top_processes = collect_average_processes(ssh, samples, interval_seconds).await?;
    let docker_stats = collect_average_docker_stats(ssh, samples, interval_seconds).await?;

    Ok(HostSnapshot {
        os_name: empty_as_unknown(&os_name),
        load_average: empty_as_unknown(&load_average),
        pressure_summary: empty_as_unknown(&pressure_summary),
        memory_summary: empty_as_unknown(&memory_summary),
        sampling_summary,
        ssh_sessions_summary: empty_as_unknown(&ssh_sessions_summary),
        ssh_recent_summary: empty_as_unknown(&ssh_recent_summary),
        swap_summary: empty_as_unknown(&swap_summary),
        sysctl_summary: empty_as_unknown(&sysctl_summary),
        thp_summary: empty_as_unknown(&thp_summary),
        docker_runtime_summary: empty_as_unknown(&docker_runtime_summary),
        top_processes: empty_as_unknown(&top_processes),
        docker_stats: empty_as_unknown(&docker_stats),
        cpu_count,
    })
}

async fn ensure_swap(
    ssh: &SshClient,
    swap_gb: u16,
) -> std::result::Result<String, CoolifyError> {
    let command = format!("sh -lc {}", sh_quote(&build_swap_script(swap_gb)));
    let result = ssh.execute(&command).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo asegurar swap en el host: {}{}",
            result.stdout, result.stderr
        )));
    }

    let details = result
        .stdout
        .trim()
        .lines()
        .last()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| "swap activa".to_string());
    Ok(format!("Swap asegurada: {details}"))
}

async fn ensure_sysctl_profile(
    ssh: &SshClient,
    swappiness: u8,
    vfs_cache_pressure: u16,
    overcommit_memory: u8,
) -> std::result::Result<String, CoolifyError> {
    let content = format!(
        "vm.swappiness={}\nvm.vfs_cache_pressure={}\nvm.overcommit_memory={}\n",
        swappiness, vfs_cache_pressure, overcommit_memory
    );
    let script = format!(
        "set -e\nmkdir -p /etc/sysctl.d\nprintf %s {} > {}\nif ! sysctl --system >/dev/null 2>&1; then sysctl -p {} >/dev/null 2>&1; fi\necho SYSCTL_READY",
        sh_quote(&content), SYSCTL_CONFIG_PATH, SYSCTL_CONFIG_PATH
    );
    let command = format!("sh -lc {}", sh_quote(&script));
    let result = ssh.execute(&command).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo aplicar sysctl host-level: {}{}",
            result.stdout, result.stderr
        )));
    }
    Ok(format!(
        "Sysctl persistido en {} (vm.swappiness={}, vm.vfs_cache_pressure={}, vm.overcommit_memory={}).",
        SYSCTL_CONFIG_PATH, swappiness, vfs_cache_pressure, overcommit_memory
    ))
}

async fn ensure_thp_disabled(ssh: &SshClient) -> std::result::Result<String, CoolifyError> {
    let service_content = "[Unit]\nDescription=Disable Transparent Huge Pages\nAfter=local-fs.target\n\n[Service]\nType=oneshot\nExecStart=/bin/sh -c 'echo never > /sys/kernel/mm/transparent_hugepage/enabled; echo never > /sys/kernel/mm/transparent_hugepage/defrag'\n\n[Install]\nWantedBy=multi-user.target\n";
    let script = format!(
        "set -e\nif [ -f /sys/kernel/mm/transparent_hugepage/enabled ]; then echo never > /sys/kernel/mm/transparent_hugepage/enabled; fi\nif [ -f /sys/kernel/mm/transparent_hugepage/defrag ]; then echo never > /sys/kernel/mm/transparent_hugepage/defrag; fi\nprintf %s {} > {}\nchmod 644 {}\nsystemctl daemon-reload\nsystemctl enable --now cm-disable-thp.service >/dev/null 2>&1\necho THP_READY",
        sh_quote(service_content), THP_SERVICE_PATH, THP_SERVICE_PATH
    );
    let result = ssh.execute(&format!("bash -lc {}", sh_quote(&script))).await?;
    if !result.success() || !result.stdout.contains("THP_READY") {
        return Err(CoolifyError::Validation(format!(
            "No se pudo desactivar THP: {}{}",
            result.stdout, result.stderr
        )));
    }
    Ok(format!(
        "THP desactivado en runtime y persistido via {}.",
        THP_SERVICE_PATH
    ))
}

async fn ensure_docker_live_restore(
    ssh: &SshClient,
) -> std::result::Result<String, CoolifyError> {
    let script = format!(
        "set -e\nmkdir -p /etc/docker\npython3 -c {}\nif systemctl reload docker >/dev/null 2>&1; then echo DOCKER_RELOADED; else echo DOCKER_RELOAD_PENDING; fi",
        sh_quote("import json, pathlib; p=pathlib.Path('/etc/docker/daemon.json'); data=json.loads(p.read_text()) if p.exists() and p.stat().st_size else {}; data['live-restore']=True; p.write_text(json.dumps(data, indent=2)+'\\n')")
    );
    let result = ssh.execute(&format!("bash -lc {}", sh_quote(&script))).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo persistir live-restore en Docker: {}{}",
            result.stdout, result.stderr
        )));
    }
    let outcome = if result.stdout.contains("DOCKER_RELOADED") {
        "config persistida y daemon recargado"
    } else {
        "config persistida; Docker requerira restart/reboot controlado para activarse"
    };
    Ok(format!(
        "Docker live-restore en {}: {}.",
        DOCKER_DAEMON_CONFIG_PATH, outcome
    ))
}

fn build_swap_script(swap_gb: u16) -> String {
    let swap_mb = u32::from(swap_gb) * 1024;
    let required_free_mb = swap_mb + 1024;

    format!(
        "set -e\nfree_mb=$(df -Pm / | awk 'NR==2 {{print $4}}')\nif swapon --show --noheadings | grep -q .; then\n  swapon --show --noheadings --output NAME,SIZE,USED,PRIO\n  exit 0\nfi\nif [ \"${{free_mb:-0}}\" -lt \"{required_free_mb}\" ]; then\n  echo INSUFFICIENT_DISK free_mb=${{free_mb:-0}} required_mb={required_free_mb}\n  exit 1\nfi\nif [ -f {SWAPFILE_PATH} ]; then\n  swapoff {SWAPFILE_PATH} 2>/dev/null || true\n  rm -f {SWAPFILE_PATH}\nfi\nif command -v fallocate >/dev/null 2>&1; then\n  fallocate -l {swap_mb}M {SWAPFILE_PATH} || dd if=/dev/zero of={SWAPFILE_PATH} bs=1M count={swap_mb} status=none\nelse\n  dd if=/dev/zero of={SWAPFILE_PATH} bs=1M count={swap_mb} status=none\nfi\nchmod 600 {SWAPFILE_PATH}\nmkswap {SWAPFILE_PATH} >/dev/null\nswapon {SWAPFILE_PATH}\ngrep -qE '^[^#]*\\s{SWAPFILE_PATH}\\s+none\\s+swap\\s+' /etc/fstab || printf '%s\\n' '{SWAPFILE_PATH} none swap sw 0 0' >> /etc/fstab\nswapon --show --noheadings --output NAME,SIZE,USED,PRIO"
    )
}

fn build_recommendations(
    before: &HostSnapshot,
    after: &HostSnapshot,
    request: &HostOptimizationRequest,
) -> Vec<String> {
    let mut recommendations = Vec::new();
    let load_1m = after
        .load_average
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(0.0);

    if !swap_is_active(&after.swap_summary) {
        recommendations.push(format!(
            "El host sigue sin swap activa. Si no es un dry-run, revisar permisos o espacio y volver a ejecutar con --swap-gb {}.",
            request.swap_gb
        ));
    } else if !swap_is_active(&before.swap_summary) {
        recommendations.push(
            "La swap ya quedo activa; sirve como colchon para picos de memoria, pero no sustituye una correccion de CPU/I/O.".to_string(),
        );
    }

    if load_1m > after.cpu_count as f32 {
        recommendations.push(format!(
            "El load ({load_1m:.2}) sigue por encima de los {} vCPU; toca revisar workers/colas o mover carga fuera del host.",
            after.cpu_count
        ));
    }

    if after.pressure_summary.contains("io_full[avg10=")
        && !after.pressure_summary.contains("io_full[avg10=0.00")
    {
        recommendations.push(
            "Sigue habiendo presion de I/O en el host; optimizar WordPress no va a eliminar por si solo una latencia de disco del nodo.".to_string(),
        );
    }

    if after.top_processes.contains("php")
        || after.top_processes.contains("redis")
        || after.top_processes.contains("node")
    {
        recommendations.push(
            "Usa la foto de procesos calientes para bajar concurrencia de workers/scheduler o separar workloads ruidosos del mismo VPS.".to_string(),
        );
    }

    if request.disable_thp && !thp_is_disabled(&after.thp_summary) {
        recommendations.push(
            "THP no quedo realmente en never; revisar permisos de sysfs o el servicio cm-disable-thp.service.".to_string(),
        );
    }

    if request.docker_live_restore && !docker_live_restore_enabled(&after.docker_runtime_summary) {
        recommendations.push(
            "live-restore quedo persistido solo a nivel de config o sigue inactivo; conviene validarlo tras el siguiente restart controlado de Docker o reboot del host.".to_string(),
        );
    }

    if recommendations.is_empty() {
        recommendations.push(
            "Host estable tras el ajuste base; repetir medicion de latencia de la app y solo luego decidir si hace falta migrar carga.".to_string(),
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

async fn exec_script(ssh: &SshClient, script: &str) -> std::result::Result<String, CoolifyError> {
    let command = format!("timeout 10 sh -lc {}", sh_quote(script));
    exec_trim(ssh, &command).await
}

async fn exec_script_timeout(
    ssh: &SshClient,
    script: &str,
    timeout_seconds: u32,
) -> std::result::Result<String, CoolifyError> {
    let command = format!("timeout {} sh -lc {}", timeout_seconds, sh_quote(script));
    exec_trim(ssh, &command).await
}

async fn collect_average_processes(
    ssh: &SshClient,
    samples: u8,
    interval_seconds: u8,
) -> std::result::Result<String, CoolifyError> {
    let script = format!(
        r#"samples={samples}; interval={interval}; i=1; while [ "$i" -le "$samples" ]; do ps -eo comm=,%cpu= --sort=-%cpu | awk '$1 !~ /^(ps|awk|sh|bash|timeout|sshd|htop|top)$/ && $1 !~ /^runc/ && $1 !~ /^containerd/ {{ print $1 "|" $2 }}'; if [ "$i" -lt "$samples" ]; then sleep "$interval"; fi; i=$((i+1)); done | awk -F'|' -v total="$samples" '{{ sum[$1]+=$2 }} END {{ for (name in sum) printf "%s %.2f\n", name, sum[name] / total }}' | sort -k2 -nr | head -7 | awk '{{ printf "%s avg_cpu=%s%%; ", $1, $2 }} END {{ if (NR == 0) printf "no-user-hotspots" }}'"#,
        samples = sanitize_sample_count(samples),
        interval = interval_seconds,
    );
    exec_script_timeout(ssh, &script, sampling_timeout_seconds(samples, interval_seconds)).await
}

async fn collect_average_docker_stats(
    ssh: &SshClient,
    samples: u8,
    interval_seconds: u8,
) -> std::result::Result<String, CoolifyError> {
    let script = format!(
        r#"samples={samples}; interval={interval}; if ! command -v docker >/dev/null 2>&1; then echo docker-unavailable; exit 0; fi; i=1; while [ "$i" -le "$samples" ]; do docker stats --no-stream --format '{{{{.Name}}}}|{{{{.CPUPerc}}}}' 2>/dev/null | sed 's/%$//'; if [ "$i" -lt "$samples" ]; then sleep "$interval"; fi; i=$((i+1)); done | awk -F'|' -v total="$samples" '{{ sum[$1]+=$2 }} END {{ for (name in sum) printf "%s %.2f\n", name, sum[name] / total }}' | sort -k2 -nr | head -8 | awk '{{ printf "%s avg_cpu=%s%%; ", $1, $2 }} END {{ if (NR == 0) printf "docker-unavailable" }}'"#,
        samples = sanitize_sample_count(samples),
        interval = interval_seconds,
    );
    exec_script_timeout(ssh, &script, sampling_timeout_seconds(samples, interval_seconds)).await
}

fn sanitize_sample_count(samples: u8) -> u8 {
    if samples == 0 { 1 } else { samples }
}

fn build_sampling_summary(samples: u8, interval_seconds: u8) -> String {
    let total_samples = sanitize_sample_count(samples);
    if total_samples <= 1 {
        return "1 muestra instantanea".to_string();
    }

    let wait_seconds = u32::from(total_samples.saturating_sub(1)) * u32::from(interval_seconds);
    format!(
        "{} muestras cada {}s (ventana ~{}s)",
        total_samples, interval_seconds, wait_seconds
    )
}

fn sampling_timeout_seconds(samples: u8, interval_seconds: u8) -> u32 {
    let total_samples = sanitize_sample_count(samples);
    let wait_seconds = u32::from(total_samples.saturating_sub(1)) * u32::from(interval_seconds);
    wait_seconds + 30
}

fn parse_sysctl_values(summary: &str) -> (u8, u16, u8) {
    let swappiness = extract_value(summary, "vm.swappiness=")
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(0);
    let vfs_cache_pressure = extract_value(summary, "vm.vfs_cache_pressure=")
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(0);
    let overcommit_memory = extract_value(summary, "vm.overcommit_memory=")
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(0);
    (swappiness, vfs_cache_pressure, overcommit_memory)
}

fn extract_value<'a>(summary: &'a str, needle: &str) -> Option<&'a str> {
    summary
        .split_whitespace()
        .find_map(|part| part.strip_prefix(needle))
}

fn swap_is_active(summary: &str) -> bool {
    !summary.is_empty() && summary != "inactive" && summary != "unknown"
}

fn thp_is_disabled(summary: &str) -> bool {
    summary.contains("[never]") || summary.contains("enabled=never")
}

fn docker_live_restore_enabled(summary: &str) -> bool {
    summary.contains("live_restore_runtime=true")
        || (summary.contains("live_restore_runtime=unknown")
            && summary.contains("live_restore_config=true"))
}

fn empty_as_unknown(value: &str) -> String {
    if value.trim().is_empty() {
        "unknown".to_string()
    } else {
        value.trim().to_string()
    }
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}