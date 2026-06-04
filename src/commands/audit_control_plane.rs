use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::control_plane_audit_manager::{self, ControlPlaneAuditReport};

use std::path::Path;
use tokio::time::{sleep, Duration};

pub async fn execute(
    config_path: &Path,
    target_name: Option<&str>,
    since: &str,
    repair: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;

    if repair {
        let repair_report = match target_name {
            Some(name) => {
                let target = settings.get_target(name)?;
                control_plane_audit_manager::repair_target(target).await?
            }
            None => control_plane_audit_manager::repair_default_vps(&settings).await?,
        };

        for step in repair_report {
            println!("Repair: {step}");
        }
    }

    let report = if repair {
        audit_report_with_retry(&settings, target_name, since).await?
    } else {
        load_audit_report(&settings, target_name, since).await?
    };

    println!("Target: {}", report.target);
    println!("Load: {}", report.load_average);
    println!("Dominancia: {}", report.dominance_summary);
    println!("Contenedores control-plane: {}", report.container_summary);
    println!("Procesos coolify: {}", report.coolify_process_summary);
    println!("Servicios coolify: {}", report.supervisor_summary);
    println!("Scheduler: {}", report.scheduler_summary);
    println!("Horizon: {}", report.horizon_summary);
    println!("Failed jobs: {}", report.failed_job_summary);
    println!("Redis: {}", report.redis_summary);
    println!("Colas: {}", report.queue_summary);
    println!("Logs coolify: {}", report.logs_summary);
    for recommendation in report.recommendations {
        println!("- {recommendation}");
    }

    Ok(())
}

async fn load_audit_report(
    settings: &Settings,
    target_name: Option<&str>,
    since: &str,
) -> std::result::Result<ControlPlaneAuditReport, CoolifyError> {
    match target_name {
        Some(name) => {
            let target = settings.get_target(name)?;
            control_plane_audit_manager::audit_target(target, since).await
        }
        None => control_plane_audit_manager::audit_default_vps(settings, since).await,
    }
}

async fn audit_report_with_retry(
    settings: &Settings,
    target_name: Option<&str>,
    since: &str,
) -> std::result::Result<ControlPlaneAuditReport, CoolifyError> {
    let mut last_error = None;

    for attempt in 0..3 {
        match load_audit_report(settings, target_name, since).await {
            Ok(report) => return Ok(report),
            Err(error) if is_transient_ssh_error(&error) && attempt < 2 => {
                last_error = Some(error);
                sleep(Duration::from_secs(2)).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        CoolifyError::Validation(
            "audit-control-plane retry agotado sin error capturado".to_string(),
        )
    }))
}

fn is_transient_ssh_error(error: &CoolifyError) -> bool {
    matches!(error, CoolifyError::Ssh(_))
}
