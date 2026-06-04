use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::host_optimization_manager::{self, HostOptimizationRequest};

use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub async fn execute(
    config_path: &Path,
    target_name: Option<&str>,
    swap_gb: u16,
    swappiness: u8,
    vfs_cache_pressure: u16,
    overcommit_memory: u8,
    disable_thp: bool,
    docker_live_restore: bool,
    dry_run: bool,
    samples: u8,
    interval_seconds: u8,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let request = HostOptimizationRequest {
        swap_gb,
        swappiness,
        vfs_cache_pressure,
        overcommit_memory,
        disable_thp,
        docker_live_restore,
        dry_run,
        samples,
        interval_seconds,
    };

    let report = match target_name {
        Some(name) => {
            let target = settings.get_target(name)?;
            host_optimization_manager::optimize_target(target, &request).await?
        }
        None => host_optimization_manager::optimize_default_vps(&settings, &request).await?,
    };

    println!("Target: {}", report.target);
    println!("SO: {}", report.os_name);
    println!("Load: {}", report.load_average);
    println!("Pressure: {}", report.pressure_summary);
    println!("Memoria: {}", report.memory_summary);
    println!("Muestreo CPU: {}", report.sampling_summary);
    println!("SSH activas: {}", report.ssh_sessions_summary);
    println!("SSH recientes: {}", report.ssh_recent_summary);
    println!("Swap antes: {}", report.swap_before);
    println!("Swap despues: {}", report.swap_after);
    println!("Sysctl: {}", report.sysctl_summary);
    println!("THP: {}", report.thp_summary);
    println!("Docker runtime: {}", report.docker_runtime_summary);
    println!("Procesos CPU promedio: {}", report.top_processes);
    println!("Docker CPU promedio: {}", report.docker_stats);
    for step in report.applied_steps {
        println!("- {step}");
    }
    for recommendation in report.recommendations {
        println!("- {recommendation}");
    }

    Ok(())
}
