/* Split 119A-5 de host_optimization_manager.rs — orquestación pública.
 * Código verbatim del original; solo cambia el origen de los ayudantes. */

use super::aplicacion::{
    build_recommendations, ensure_docker_live_restore, ensure_swap, ensure_sysctl_profile,
    ensure_thp_disabled,
};
use super::diagnostico::{
    collect_snapshot, docker_live_restore_enabled, parse_sysctl_values, swap_is_active,
    thp_is_disabled,
};
use super::tipos::{
    HostOptimizationReport, HostOptimizationRequest, DOCKER_DAEMON_CONFIG_PATH, SWAPFILE_PATH,
    SYSCTL_CONFIG_PATH, THP_SERVICE_PATH,
};
use crate::config::{DeploymentTargetConfig, Settings, VpsConfig};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

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
            applied_steps.push(
                "Docker ya reporta live-restore habilitado; no se toco daemon.json.".to_string(),
            );
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
