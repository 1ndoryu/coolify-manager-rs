/* Split 119A-5 de maintenance_window_manager.rs — evaluación de la ventana.
 * Código verbatim del original; validate_policy y resolve_sample_sites se
 * comparten con programacion.rs vía pub(super). */

use super::deriva::{
    active_critical_ops, collect_drift_snapshots, detect_installed_kernel, detect_reboot_required,
    evaluate_drift, reboot_frequency_allows_reboot,
};
use super::sondas::exec_trim;
use super::tipos::{MaintenanceSiteHealth, MaintenanceWindowReport, MaintenanceWindowRequest};
use crate::config::{DeploymentTargetConfig, MaintenancePolicyConfig, RebootPolicy, Settings};
use crate::domain::SiteConfig;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::services::{health_manager, host_maintenance_manager};

pub async fn evaluate_default_vps(
    settings: &Settings,
    request: &MaintenanceWindowRequest,
) -> std::result::Result<MaintenanceWindowReport, CoolifyError> {
    let target = settings.default_target();
    evaluate_target(settings, &target, request).await
}

pub async fn evaluate_target(
    settings: &Settings,
    target: &DeploymentTargetConfig,
    request: &MaintenanceWindowRequest,
) -> std::result::Result<MaintenanceWindowReport, CoolifyError> {
    let policy = target.maintenance_policy.as_ref().ok_or_else(|| {
        CoolifyError::Validation(format!(
            "Target '{}' sin maintenancePolicy; no hay politica que evaluar",
            target.name
        ))
    })?;

    if !policy.enabled && !request.force_evaluate {
        return Ok(MaintenanceWindowReport {
            target: target.name.clone(),
            reboot_policy: policy.reboot_policy.to_string(),
            decision: "blocked".to_string(),
            blocked: true,
            reboot_required: false,
            drift_detected: false,
            running_kernel: "unknown".to_string(),
            installed_kernel: "unknown".to_string(),
            load_average: "unknown".to_string(),
            cpu_pressure: "unknown".to_string(),
            io_pressure: "unknown".to_string(),
            control_plane_cpu_percent: 0.0,
            critical_ops_summary: "maintenance-policy-disabled".to_string(),
            applied_maintenance: false,
            reboot_scheduled: false,
            sample_sites: Vec::new(),
            notes: vec![
                "La politica existe pero esta deshabilitada; usa --force-evaluate para simular la decision.".to_string(),
            ],
        });
    }

    validate_policy(target, policy)?;

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let running_kernel = exec_trim(&ssh, "uname -r").await?;
    let installed_kernel = detect_installed_kernel(&ssh).await?;
    let reboot_required = detect_reboot_required(&ssh, &running_kernel, &installed_kernel).await?;
    let load_average = exec_trim(&ssh, "awk '{print $1, $2, $3}' /proc/loadavg").await?;
    let cpu_pressure = exec_trim(
        &ssh,
        "awk -F'avg10=' '/some/ {split($2,a,\" \"); print \"cpu_some avg10=\" a[1]}' /proc/pressure/cpu",
    )
    .await?;
    let io_pressure = exec_trim(
        &ssh,
        "awk -F'avg10=' '/full/ {split($2,a,\" \"); print \"io_full avg10=\" a[1]}' /proc/pressure/io",
    )
    .await?;
    let critical_ops_summary = active_critical_ops(&ssh).await?;
    let snapshots = collect_drift_snapshots(&ssh, &policy.drift_rules).await?;
    let drift_detected = evaluate_drift(&snapshots, &policy.drift_rules);
    let control_plane_cpu_percent = snapshots
        .last()
        .map(|snapshot| snapshot.control_plane_cpu_percent)
        .unwrap_or_default();

    let sample_sites = resolve_sample_sites(settings, target, policy)?;
    let sample_health = run_sample_site_health(settings, &ssh, &sample_sites).await;
    let healthy_samples = sample_health.iter().all(|site| site.healthy);
    let blocked_by_ops = critical_ops_summary != "none";
    let blocked = blocked_by_ops || !healthy_samples;

    let mut notes = Vec::new();
    if blocked_by_ops {
        notes.push(format!(
            "Se detectaron operaciones criticas activas en el host: {}",
            critical_ops_summary
        ));
    }
    if !healthy_samples {
        notes.push(
            "Uno o mas sampleSites no estan sanos; la ventana automatica queda bloqueada para evitar agravar un incidente.".to_string(),
        );
    }

    let should_reboot = match policy.reboot_policy {
        RebootPolicy::ManualOnly => false,
        RebootPolicy::IfRequired => reboot_required,
        RebootPolicy::IfDriftDetected => {
            reboot_required
                || (drift_detected && reboot_frequency_allows_reboot(&ssh, policy).await?)
        }
    };

    let decision = if blocked {
        "blocked"
    } else if should_reboot {
        "maintain-and-reboot"
    } else {
        "maintain-no-reboot"
    }
    .to_string();

    let mut applied_maintenance = false;
    let mut reboot_scheduled = false;
    if request.apply && !blocked {
        let maintenance_report = host_maintenance_manager::maintain_target(
            target,
            &host_maintenance_manager::HostMaintenanceRequest {
                reboot: should_reboot,
                dry_run: request.dry_run,
            },
        )
        .await?;
        applied_maintenance = true;
        reboot_scheduled = maintenance_report.reboot_scheduled;
        notes.extend(maintenance_report.applied_steps);
        notes.extend(maintenance_report.recommendations);
    } else if !request.apply {
        notes.push("Evaluacion completada sin aplicar mantenimiento; usa --apply para ejecutar la decision.".to_string());
    }

    if request.dry_run {
        notes.push("Dry run activo: la decision es real, pero no se muta el host.".to_string());
    }

    Ok(MaintenanceWindowReport {
        target: target.name.clone(),
        reboot_policy: policy.reboot_policy.to_string(),
        decision,
        blocked,
        reboot_required,
        drift_detected,
        running_kernel,
        installed_kernel,
        load_average,
        cpu_pressure,
        io_pressure,
        control_plane_cpu_percent,
        critical_ops_summary,
        applied_maintenance,
        reboot_scheduled,
        sample_sites: sample_health,
        notes,
    })
}

pub(super) fn validate_policy(
    target: &DeploymentTargetConfig,
    policy: &MaintenancePolicyConfig,
) -> std::result::Result<(), CoolifyError> {
    if policy.timezone.trim().is_empty() {
        return Err(CoolifyError::Validation(format!(
            "Target '{}' con maintenancePolicy sin timezone explicita",
            target.name
        )));
    }
    if policy.window_start_local.trim().is_empty() {
        return Err(CoolifyError::Validation(format!(
            "Target '{}' con maintenancePolicy sin windowStartLocal",
            target.name
        )));
    }
    Ok(())
}

pub(super) fn resolve_sample_sites<'a>(
    settings: &'a Settings,
    target: &DeploymentTargetConfig,
    policy: &MaintenancePolicyConfig,
) -> std::result::Result<Vec<&'a SiteConfig>, CoolifyError> {
    if !policy.sample_sites.is_empty() {
        return policy
            .sample_sites
            .iter()
            .map(|site_name| settings.get_site(site_name))
            .collect();
    }

    let mut sites = Vec::new();
    for site in &settings.sitios {
        let resolved_target = settings.resolve_site_target(site)?;
        if resolved_target.name == target.name {
            sites.push(site);
        }
    }
    Ok(sites)
}

async fn run_sample_site_health(
    settings: &Settings,
    ssh: &SshClient,
    sites: &[&SiteConfig],
) -> Vec<MaintenanceSiteHealth> {
    let mut reports = Vec::new();
    for site in sites {
        match health_manager::run_site_health_check(settings, site, ssh).await {
            Ok(report) => {
                let healthy = report.healthy();
                reports.push(MaintenanceSiteHealth {
                    site_name: report.site_name,
                    healthy,
                    details: report.details,
                })
            }
            Err(error) => reports.push(MaintenanceSiteHealth {
                site_name: site.nombre.clone(),
                healthy: false,
                details: vec![error.to_string()],
            }),
        }
    }
    reports
}
