use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::host_optimization_manager::{self, HostOptimizationRequest};

use std::path::Path;

/* Params del comando optimize-host (119A-6). Todo Copy: `= *p` sin mover. */
#[derive(Clone, Copy)]
pub struct ParamsOptimizeHost<'a> {
    pub config_path: &'a Path,
    pub target_name: Option<&'a str>,
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

pub async fn execute(p: &ParamsOptimizeHost<'_>) -> std::result::Result<(), CoolifyError> {
    let ParamsOptimizeHost {
        config_path,
        target_name,
        swap_gb,
        swappiness,
        vfs_cache_pressure,
        overcommit_memory,
        disable_thp,
        docker_live_restore,
        dry_run,
        samples,
        interval_seconds,
    } = *p;
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
