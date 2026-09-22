/* Split 119A-5 de maintenance_window_manager.rs — deteccion de drift y reboot.
 * Código verbatim del original; solo cambia visibilidad a pub(super). */

use super::sondas::{exec_trim, parse_f32, shell_single_quote};
use super::tipos::DriftSnapshot;
use crate::config::{DriftRulesConfig, MaintenancePolicyConfig};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use super::super::maintenance_render::SNAPSHOT_INTERVAL_SECS;

pub(super) async fn collect_drift_snapshots(
    ssh: &SshClient,
    rules: &DriftRulesConfig,
) -> std::result::Result<Vec<DriftSnapshot>, CoolifyError> {
    let sample_count = rules.required_consecutive_snapshots.max(1);
    let mut samples = Vec::new();
    for index in 0..sample_count {
        samples.push(collect_snapshot(ssh).await?);
        if index + 1 < sample_count {
            tokio::time::sleep(std::time::Duration::from_secs(SNAPSHOT_INTERVAL_SECS)).await;
        }
    }
    Ok(samples)
}

pub(super) async fn collect_snapshot(
    ssh: &SshClient,
) -> std::result::Result<DriftSnapshot, CoolifyError> {
    let load15 = parse_f32(
        &exec_trim(ssh, "awk '{print $3}' /proc/loadavg").await?,
        0.0,
    );
    let cpu_count = exec_trim(ssh, "nproc").await?.parse::<u32>().unwrap_or(1);
    let cpu_psi_some_avg10 = parse_f32(
        &exec_trim(
            ssh,
            "awk -F'avg10=' '/some/ {split($2,a,\" \"); print a[1]}' /proc/pressure/cpu",
        )
        .await?,
        0.0,
    );
    let io_psi_full_avg10 = parse_f32(
        &exec_trim(
            ssh,
            "awk -F'avg10=' '/full/ {split($2,a,\" \"); print a[1]}' /proc/pressure/io",
        )
        .await?,
        0.0,
    );
    let control_plane_cpu_script = r#"if command -v docker >/dev/null 2>&1; then docker stats --no-stream --format "{{.Name}}|{{.CPUPerc}}" 2>/dev/null | awk -F"|" '$1 ~ /^coolify/ {gsub(/%/, "", $2); sum += $2 + 0} END {printf "%.2f", sum + 0}'; else echo 0; fi"#;
    let control_plane_cpu_percent = parse_f32(
        &exec_trim(
            ssh,
            &format!("bash -lc {}", shell_single_quote(control_plane_cpu_script)),
        )
        .await?,
        0.0,
    );

    Ok(DriftSnapshot {
        load15,
        cpu_count,
        cpu_psi_some_avg10,
        io_psi_full_avg10,
        control_plane_cpu_percent,
    })
}

pub(super) fn evaluate_drift(samples: &[DriftSnapshot], rules: &DriftRulesConfig) -> bool {
    if samples.is_empty() {
        return false;
    }

    samples.iter().all(|sample| {
        let load_hot =
            !rules.avg15_greater_than_cpu_count || sample.load15 > sample.cpu_count as f32;
        let cpu_hot = sample.cpu_psi_some_avg10 >= rules.cpu_psi_some_avg10;
        let io_or_control_hot = sample.io_psi_full_avg10 >= rules.io_psi_full_avg10
            || sample.control_plane_cpu_percent >= rules.control_plane_cpu_percent;

        load_hot && cpu_hot && io_or_control_hot
    })
}

pub(super) async fn reboot_frequency_allows_reboot(
    ssh: &SshClient,
    policy: &MaintenancePolicyConfig,
) -> std::result::Result<bool, CoolifyError> {
    let uptime_seconds = exec_trim(ssh, "cut -d. -f1 /proc/uptime | awk '{print $1}'")
        .await?
        .parse::<u64>()
        .unwrap_or(u64::MAX);
    let min_seconds = match policy.max_reboot_frequency.trim() {
        "daily" => 86_400,
        "weekly" => 604_800,
        "monthly" => 2_592_000,
        _ => 0,
    };

    Ok(min_seconds == 0 || uptime_seconds >= min_seconds)
}

pub(super) async fn active_critical_ops(
    ssh: &SshClient,
) -> std::result::Result<String, CoolifyError> {
    let result = exec_trim(
        ssh,
        "bash -lc 'pgrep -af \"apt|apt-get|dpkg|docker build|docker compose build|docker compose up|pg_restore|mysqldump|rsync|git clone|cargo build\" 2>/dev/null | grep -v \"pgrep -af\" | grep -v \"check-maintenance-window\" | head -n 5 | tr \"\n\" \";\" || true'",
    )
    .await?;
    Ok(if result.trim().is_empty() {
        "none".to_string()
    } else {
        result
    })
}

pub(super) async fn detect_installed_kernel(
    ssh: &SshClient,
) -> std::result::Result<String, CoolifyError> {
    exec_trim(
        ssh,
        "bash -lc 'dpkg-query -W -f=${Version} linux-image-generic 2>/dev/null || dpkg-query -W -f=${Version} linux-image-generic-hwe-24.04 2>/dev/null || uname -r'",
    )
    .await
}

pub(super) async fn detect_reboot_required(
    ssh: &SshClient,
    running_kernel: &str,
    installed_kernel: &str,
) -> std::result::Result<bool, CoolifyError> {
    let reboot_file = exec_trim(
        ssh,
        "bash -lc 'if [ -f /var/run/reboot-required ]; then echo yes; else echo no; fi'",
    )
    .await?;
    Ok(reboot_file == "yes"
        || (!installed_kernel.is_empty()
            && installed_kernel != "unknown"
            && !running_kernel.is_empty()
            && !installed_kernel.contains(running_kernel)))
}
