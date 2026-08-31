/*
 * Render de artefactos del maintenance window: unidades systemd (service/timer)
 * y el script bash remoto que ejecuta el upgrade con guardas de drift.
 * Extraido de maintenance_window_manager.rs (Fase H): render puro (sin I/O)
 * para mantener el manager bajo el limite de lineas.
 */

use crate::config::{DeploymentTargetConfig, MaintenancePolicyConfig};
use crate::domain::SiteConfig;

pub(crate) const SNAPSHOT_INTERVAL_SECS: u64 = 5;

pub(crate) fn render_service_unit(target_name: &str, script_path: &str) -> String {
    format!(
        "[Unit]\nDescription=Coolify Manager maintenance window for {target_name}\nAfter=network-online.target docker.service\nWants=network-online.target\n\n[Service]\nType=oneshot\nUser=root\nExecStart={script_path}\n"
    )
}

pub(crate) fn render_timer_unit(
    target_name: &str,
    policy: &MaintenancePolicyConfig,
    unit_name: &str,
) -> String {
    format!(
        "[Unit]\nDescription=Daily maintenance timer for {target_name}\n\n[Timer]\nOnCalendar=*-*-* {window}\nTimeZone={timezone}\nRandomizedDelaySec={delay}\nPersistent=true\nAccuracySec=1m\nUnit={unit}.service\n\n[Install]\nWantedBy=timers.target\n",
        window = policy.window_start_local,
        timezone = policy.timezone,
        delay = policy.randomized_delay,
        unit = unit_name,
    )
}

pub(crate) fn render_remote_script(
    target: &DeploymentTargetConfig,
    policy: &MaintenancePolicyConfig,
    sample_sites: &[&SiteConfig],
) -> String {
    let health_checks = if sample_sites.is_empty() {
        "# no sample sites configured\n".to_string()
    } else {
        sample_sites
            .iter()
            .map(|site| {
                let path = normalize_health_path(&site.health_check.http_path);
                format!(
                    "check_health '{}' 'https://{}{}'\n",
                    site.nombre, site.dominio, path
                )
            })
            .collect::<String>()
    };
    let avg15_rule = if policy.drift_rules.avg15_greater_than_cpu_count {
        "1"
    } else {
        "0"
    };
    let reboot_policy = policy.reboot_policy.to_string();
    let max_frequency_seconds = match policy.max_reboot_frequency.trim() {
        "daily" => 86_400,
        "weekly" => 604_800,
        "monthly" => 2_592_000,
        _ => 0,
    };

    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

TARGET='{target}'
REBOOT_POLICY='{reboot_policy}'
REQUIRED_SNAPSHOTS={required_snapshots}
AVG15_RULE={avg15_rule}
CONTROL_PLANE_CPU_THRESHOLD={control_plane_cpu_threshold}
CPU_PSI_THRESHOLD={cpu_psi_threshold}
IO_PSI_THRESHOLD={io_psi_threshold}
MAX_REBOOT_FREQUENCY_SECONDS={max_frequency_seconds}
LOG_FILE='/var/log/{unit_name}.log'
LOCK_FILE='/var/run/{unit_name}.lock'

mkdir -p /var/log
exec >>"$LOG_FILE" 2>&1
exec 9>"$LOCK_FILE"
if ! flock -n 9; then
    echo "$(date -Is) target=$TARGET status=locked"
    exit 0
fi

echo "$(date -Is) target=$TARGET status=start policy=$REBOOT_POLICY"

critical_ops=$(pgrep -af "apt|apt-get|dpkg|docker build|docker compose build|docker compose up|pg_restore|mysqldump|rsync|git clone|cargo build" 2>/dev/null | grep -v "pgrep -af" | grep -v "coolify-manager-maintenance" | head -n 5 | tr '\n' ';' || true)
if [ -n "$critical_ops" ]; then
    echo "$(date -Is) target=$TARGET status=blocked reason=critical_ops ops=$critical_ops"
    exit 0
fi

health_failed=0
check_health() {{
    local name="$1"
    local url="$2"
    if ! curl -fsS --max-time 10 "$url" >/dev/null; then
        echo "$(date -Is) target=$TARGET status=blocked reason=health site=$name url=$url"
        health_failed=1
    fi
}}

{health_checks}

if [ "$health_failed" -eq 1 ]; then
    exit 0
fi

running_kernel=$(uname -r)
installed_kernel=$(bash -lc 'dpkg-query -W -f=${{Version}} linux-image-generic 2>/dev/null || dpkg-query -W -f=${{Version}} linux-image-generic-hwe-24.04 2>/dev/null || uname -r')
reboot_required=no
if [ -f /var/run/reboot-required ]; then
    reboot_required=yes
elif [[ "$installed_kernel" != *"$running_kernel"* ]]; then
    reboot_required=yes
fi

drift_hits=0
sample_index=0
while [ "$sample_index" -lt "$REQUIRED_SNAPSHOTS" ]; do
    load15=$(awk '{{print $3}}' /proc/loadavg)
    cpu_count=$(nproc)
    cpu_psi=$(awk -F'avg10=' '/some/ {{split($2,a," "); print a[1]}}' /proc/pressure/cpu)
    io_psi=$(awk -F'avg10=' '/full/ {{split($2,a," "); print a[1]}}' /proc/pressure/io)
    control_plane_cpu=$(bash -lc 'if command -v docker >/dev/null 2>&1; then docker stats --no-stream --format "{{{{.Name}}}}|{{{{.CPUPerc}}}}" 2>/dev/null | awk -F"|" '\''$1 ~ /^coolify/ {{gsub(/%/, "", $2); sum += $2 + 0}} END {{printf "%.2f", sum + 0}}'\''; else echo 0; fi')
    sample_hot=1
    if [ "$AVG15_RULE" -eq 1 ] && ! awk "BEGIN {{exit !($load15 > $cpu_count)}}"; then sample_hot=0; fi
    if ! awk "BEGIN {{exit !($cpu_psi >= $CPU_PSI_THRESHOLD)}}"; then sample_hot=0; fi
    if ! awk "BEGIN {{exit !(($io_psi >= $IO_PSI_THRESHOLD) || ($control_plane_cpu >= $CONTROL_PLANE_CPU_THRESHOLD))}}"; then sample_hot=0; fi
    if [ "$sample_hot" -eq 1 ]; then drift_hits=$((drift_hits + 1)); fi
    sample_index=$((sample_index + 1))
    [ "$sample_index" -lt "$REQUIRED_SNAPSHOTS" ] && sleep {snapshot_interval}
done

should_reboot=no
reboot_reason=none
if [ "$reboot_required" = yes ] && [ "$REBOOT_POLICY" != 'manual-only' ]; then
    should_reboot=yes
    reboot_reason=required
elif [ "$REBOOT_POLICY" = 'if-drift-detected' ] && [ "$drift_hits" -ge "$REQUIRED_SNAPSHOTS" ]; then
    should_reboot=yes
    reboot_reason=drift
fi

if [ "$should_reboot" = yes ] && [ "$reboot_reason" = drift ] && [ "$MAX_REBOOT_FREQUENCY_SECONDS" -gt 0 ]; then
    uptime_seconds=$(cut -d. -f1 /proc/uptime | awk '{{print $1}}')
    if [ "$uptime_seconds" -lt "$MAX_REBOOT_FREQUENCY_SECONDS" ]; then
        echo "$(date -Is) target=$TARGET status=no-reboot reason=frequency-guard uptime_seconds=$uptime_seconds"
        should_reboot=no
        reboot_reason=guarded
    fi
fi

export DEBIAN_FRONTEND=noninteractive
lock_wait=0
while true; do
    lock_pid=""
    if command -v fuser >/dev/null 2>&1; then
        lock_pid=$(fuser /var/lib/dpkg/lock-frontend 2>/dev/null | awk 'NR==1 {{print $1}}')
    fi
    if [ -z "$lock_pid" ]; then
        lock_pid=$(pgrep -x apt-get 2>/dev/null | head -1 || true)
    fi
    if [ -z "$lock_pid" ]; then
        break
    fi
    if [ "$lock_wait" -ge 900 ]; then
        cmd=$(tr '\0' ' ' < /proc/$lock_pid/cmdline 2>/dev/null || echo unknown)
        echo "$(date -Is) target=$TARGET status=blocked reason=apt-lock pid=$lock_pid cmd=$cmd waited=${{lock_wait}}s"
        exit 0
    fi
    sleep 15
    lock_wait=$((lock_wait + 15))
done

if dpkg --audit 2>/dev/null | grep -q .; then
    echo "$(date -Is) target=$TARGET status=dpkg-recovery"
    dpkg --configure -a
fi

apt-get update
apt-get -y full-upgrade
apt-get -y autoremove --purge
apt-get clean
remaining_upgradable=$(apt list --upgradable 2>/dev/null | sed '1d' | wc -l | tr -d ' ')
echo "$(date -Is) target=$TARGET status=maintained reboot_required=$reboot_required drift_hits=$drift_hits remaining_upgradable=$remaining_upgradable reason=$reboot_reason"

if [ "$should_reboot" = yes ]; then
    nohup sh -c "sleep 3; systemctl reboot" >/dev/null 2>&1 &
    echo "$(date -Is) target=$TARGET status=reboot-scheduled reason=$reboot_reason"
fi
"#,
        target = target.name,
        reboot_policy = reboot_policy,
        required_snapshots = policy.drift_rules.required_consecutive_snapshots.max(1),
        avg15_rule = avg15_rule,
        control_plane_cpu_threshold = policy.drift_rules.control_plane_cpu_percent,
        cpu_psi_threshold = policy.drift_rules.cpu_psi_some_avg10,
        io_psi_threshold = policy.drift_rules.io_psi_full_avg10,
        max_frequency_seconds = max_frequency_seconds,
        unit_name = unit_name(&target.name),
        health_checks = health_checks,
        snapshot_interval = SNAPSHOT_INTERVAL_SECS,
    )
}

pub(crate) fn unit_name(target_name: &str) -> String {
    format!(
        "coolify-manager-maintenance-{}",
        target_name
            .chars()
            .map(|character| if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            })
            .collect::<String>()
            .trim_matches('-')
    )
}

fn normalize_health_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        "/".to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}
