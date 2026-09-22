/* Split 119A-5 de host_optimization_manager.rs — ejecutores SSH y muestreo.
 * Código verbatim del original; solo cambia visibilidad a pub(super). */

use super::tipos::HostSnapshot;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

pub(super) async fn collect_snapshot(
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

pub(super) async fn exec_script(
    ssh: &SshClient,
    script: &str,
) -> std::result::Result<String, CoolifyError> {
    let command = format!("timeout 10 sh -lc {}", sh_quote(script));
    exec_trim(ssh, &command).await
}

pub(super) async fn exec_script_timeout(
    ssh: &SshClient,
    script: &str,
    timeout_seconds: u32,
) -> std::result::Result<String, CoolifyError> {
    let command = format!("timeout {} sh -lc {}", timeout_seconds, sh_quote(script));
    exec_trim(ssh, &command).await
}

pub(super) async fn collect_average_processes(
    ssh: &SshClient,
    samples: u8,
    interval_seconds: u8,
) -> std::result::Result<String, CoolifyError> {
    let script = format!(
        r#"samples={samples}; interval={interval}; i=1; while [ "$i" -le "$samples" ]; do ps -eo comm=,%cpu= --sort=-%cpu | awk '$1 !~ /^(ps|awk|sh|bash|timeout|sshd|htop|top)$/ && $1 !~ /^runc/ && $1 !~ /^containerd/ {{ print $1 "|" $2 }}'; if [ "$i" -lt "$samples" ]; then sleep "$interval"; fi; i=$((i+1)); done | awk -F'|' -v total="$samples" '{{ sum[$1]+=$2 }} END {{ for (name in sum) printf "%s %.2f\n", name, sum[name] / total }}' | sort -k2 -nr | head -7 | awk '{{ printf "%s avg_cpu=%s%%; ", $1, $2 }} END {{ if (NR == 0) printf "no-user-hotspots" }}'"#,
        samples = sanitize_sample_count(samples),
        interval = interval_seconds,
    );
    exec_script_timeout(
        ssh,
        &script,
        sampling_timeout_seconds(samples, interval_seconds),
    )
    .await
}

pub(super) async fn collect_average_docker_stats(
    ssh: &SshClient,
    samples: u8,
    interval_seconds: u8,
) -> std::result::Result<String, CoolifyError> {
    let script = format!(
        r#"samples={samples}; interval={interval}; if ! command -v docker >/dev/null 2>&1; then echo docker-unavailable; exit 0; fi; i=1; while [ "$i" -le "$samples" ]; do docker stats --no-stream --format '{{{{.Name}}}}|{{{{.CPUPerc}}}}' 2>/dev/null | sed 's/%$//'; if [ "$i" -lt "$samples" ]; then sleep "$interval"; fi; i=$((i+1)); done | awk -F'|' -v total="$samples" '{{ sum[$1]+=$2 }} END {{ for (name in sum) printf "%s %.2f\n", name, sum[name] / total }}' | sort -k2 -nr | head -8 | awk '{{ printf "%s avg_cpu=%s%%; ", $1, $2 }} END {{ if (NR == 0) printf "docker-unavailable" }}'"#,
        samples = sanitize_sample_count(samples),
        interval = interval_seconds,
    );
    exec_script_timeout(
        ssh,
        &script,
        sampling_timeout_seconds(samples, interval_seconds),
    )
    .await
}

pub(super) fn sanitize_sample_count(samples: u8) -> u8 {
    if samples == 0 {
        1
    } else {
        samples
    }
}

pub(super) fn build_sampling_summary(samples: u8, interval_seconds: u8) -> String {
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

pub(super) fn sampling_timeout_seconds(samples: u8, interval_seconds: u8) -> u32 {
    let total_samples = sanitize_sample_count(samples);
    let wait_seconds = u32::from(total_samples.saturating_sub(1)) * u32::from(interval_seconds);
    wait_seconds + 30
}

pub(super) fn parse_sysctl_values(summary: &str) -> (u8, u16, u8) {
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

pub(super) fn extract_value<'a>(summary: &'a str, needle: &str) -> Option<&'a str> {
    summary
        .split_whitespace()
        .find_map(|part| part.strip_prefix(needle))
}

pub(super) fn swap_is_active(summary: &str) -> bool {
    !summary.is_empty() && summary != "inactive" && summary != "unknown"
}

pub(super) fn thp_is_disabled(summary: &str) -> bool {
    summary.contains("[never]") || summary.contains("enabled=never")
}

pub(super) fn docker_live_restore_enabled(summary: &str) -> bool {
    summary.contains("live_restore_runtime=true")
        || (summary.contains("live_restore_runtime=unknown")
            && summary.contains("live_restore_config=true"))
}

pub(super) fn empty_as_unknown(value: &str) -> String {
    if value.trim().is_empty() {
        "unknown".to_string()
    } else {
        value.trim().to_string()
    }
}

pub(super) fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
