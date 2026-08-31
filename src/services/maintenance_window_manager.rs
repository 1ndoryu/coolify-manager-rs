use crate::config::{
    DeploymentTargetConfig, DriftRulesConfig, MaintenancePolicyConfig, RebootPolicy, Settings,
};
use crate::domain::SiteConfig;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::services::{health_manager, host_maintenance_manager};

use serde::Serialize;

use super::maintenance_render::{
    render_remote_script, render_service_unit, render_timer_unit, unit_name, SNAPSHOT_INTERVAL_SECS,
};

#[derive(Debug, Clone)]
pub struct MaintenanceWindowRequest {
    pub apply: bool,
    pub dry_run: bool,
    pub force_evaluate: bool,
}

#[derive(Debug, Clone)]
pub struct ScheduleMaintenanceRequest {
    pub dry_run: bool,
    pub remove: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaintenanceSiteHealth {
    pub site_name: String,
    pub healthy: bool,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaintenanceWindowReport {
    pub target: String,
    pub reboot_policy: String,
    pub decision: String,
    pub blocked: bool,
    pub reboot_required: bool,
    pub drift_detected: bool,
    pub running_kernel: String,
    pub installed_kernel: String,
    pub load_average: String,
    pub cpu_pressure: String,
    pub io_pressure: String,
    pub control_plane_cpu_percent: f32,
    pub critical_ops_summary: String,
    pub applied_maintenance: bool,
    pub reboot_scheduled: bool,
    pub sample_sites: Vec<MaintenanceSiteHealth>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScheduleMaintenanceReport {
    pub target: String,
    pub script_path: String,
    pub service_path: String,
    pub timer_path: String,
    pub removed: bool,
    pub next_trigger_summary: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
struct DriftSnapshot {
    load15: f32,
    cpu_count: u32,
    cpu_psi_some_avg10: f32,
    io_psi_full_avg10: f32,
    control_plane_cpu_percent: f32,
}

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

pub async fn schedule_target(
    settings: &Settings,
    target: &DeploymentTargetConfig,
    request: &ScheduleMaintenanceRequest,
) -> std::result::Result<ScheduleMaintenanceReport, CoolifyError> {
    let policy = target.maintenance_policy.as_ref().ok_or_else(|| {
        CoolifyError::Validation(format!(
            "Target '{}' sin maintenancePolicy; no hay nada que programar",
            target.name
        ))
    })?;
    validate_policy(target, policy)?;

    let unit_name = unit_name(&target.name);
    let script_path = format!("/usr/local/bin/{unit_name}.sh");
    let service_path = format!("/etc/systemd/system/{unit_name}.service");
    let timer_path = format!("/etc/systemd/system/{unit_name}.timer");
    let sample_sites = resolve_sample_sites(settings, target, policy)?;

    let script = render_remote_script(target, policy, &sample_sites);
    let service = render_service_unit(&target.name, &script_path);
    let timer = render_timer_unit(&target.name, policy, &unit_name);

    let mut notes = Vec::new();
    if request.dry_run {
        notes.push(format!(
            "Dry run: se renderizarian {} bytes de script remoto.",
            script.len()
        ));
        notes.push(format!(
            "Dry run: se instalaria un timer diario {} {}.",
            policy.window_start_local, policy.timezone
        ));
        return Ok(ScheduleMaintenanceReport {
            target: target.name.clone(),
            script_path,
            service_path,
            timer_path,
            removed: request.remove,
            next_trigger_summary: "dry-run".to_string(),
            notes,
        });
    }

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    if request.remove {
        let removal = format!(
            "bash -lc 'systemctl disable --now {unit}.timer >/dev/null 2>&1 || true; rm -f {script} {service} {timer}; systemctl daemon-reload; echo REMOVED'",
            unit = shell_single_quote(&unit_name),
            script = shell_single_quote(&script_path),
            service = shell_single_quote(&service_path),
            timer = shell_single_quote(&timer_path),
        );
        let result = ssh.execute(&removal).await?;
        if !result.success() || !result.stdout.contains("REMOVED") {
            return Err(CoolifyError::Validation(format!(
                "No se pudo retirar el scheduler remoto: {}{}",
                result.stdout, result.stderr
            )));
        }
        notes.push("Timer remoto retirado y daemon-reload ejecutado.".to_string());
        return Ok(ScheduleMaintenanceReport {
            target: target.name.clone(),
            script_path,
            service_path,
            timer_path,
            removed: true,
            next_trigger_summary: "removed".to_string(),
            notes,
        });
    }

    upload_remote_text(&ssh, &script, &script_path).await?;
    upload_remote_text(&ssh, &service, &service_path).await?;
    upload_remote_text(&ssh, &timer, &timer_path).await?;

    let install_command = format!(
        "bash -lc 'chmod 755 {script}; systemctl daemon-reload; systemctl enable --now {unit}.timer; systemctl list-timers --all {unit}.timer --no-pager --no-legend || true'",
        script = shell_single_quote(&script_path),
        unit = shell_single_quote(&unit_name),
    );
    let result = ssh.execute(&install_command).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo instalar el timer remoto: {}{}",
            result.stdout, result.stderr
        )));
    }
    notes.push("Script, service y timer instalados en el host remoto.".to_string());

    Ok(ScheduleMaintenanceReport {
        target: target.name.clone(),
        script_path,
        service_path,
        timer_path,
        removed: false,
        next_trigger_summary: empty_as_unknown(&result.stdout),
        notes,
    })
}

fn validate_policy(
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

fn resolve_sample_sites<'a>(
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

async fn collect_drift_snapshots(
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

async fn collect_snapshot(ssh: &SshClient) -> std::result::Result<DriftSnapshot, CoolifyError> {
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

fn evaluate_drift(samples: &[DriftSnapshot], rules: &DriftRulesConfig) -> bool {
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

async fn reboot_frequency_allows_reboot(
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

async fn active_critical_ops(ssh: &SshClient) -> std::result::Result<String, CoolifyError> {
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

async fn detect_installed_kernel(ssh: &SshClient) -> std::result::Result<String, CoolifyError> {
    exec_trim(
        ssh,
        "bash -lc 'dpkg-query -W -f=${Version} linux-image-generic 2>/dev/null || dpkg-query -W -f=${Version} linux-image-generic-hwe-24.04 2>/dev/null || uname -r'",
    )
    .await
}

async fn detect_reboot_required(
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

async fn exec_trim(ssh: &SshClient, command: &str) -> std::result::Result<String, CoolifyError> {
    let result = ssh.execute(command).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo ejecutando comando remoto: {}{}",
            result.stdout, result.stderr
        )));
    }
    Ok(result.stdout.trim().to_string())
}

async fn upload_remote_text(
    ssh: &SshClient,
    content: &str,
    remote_path: &str,
) -> std::result::Result<(), CoolifyError> {
    let temp_path = std::env::temp_dir().join(format!(
        "coolify-manager-{}-{}.tmp",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&temp_path, content).map_err(|error| {
        CoolifyError::Validation(format!("No se pudo crear archivo temporal: {error}"))
    })?;
    let upload_result = ssh.upload_file(&temp_path, remote_path).await;
    let _ = std::fs::remove_file(&temp_path);
    upload_result
}

fn parse_f32(value: &str, fallback: f32) -> f32 {
    value.trim().parse::<f32>().unwrap_or(fallback)
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn empty_as_unknown(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}
