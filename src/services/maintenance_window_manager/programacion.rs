/* Split 119A-5 de maintenance_window_manager.rs — programación del timer remoto.
 * Código verbatim del original; solo cambia el origen de los ayudantes. */

use super::evaluacion::{resolve_sample_sites, validate_policy};
use super::sondas::{empty_as_unknown, shell_single_quote, upload_remote_text};
use super::tipos::{ScheduleMaintenanceReport, ScheduleMaintenanceRequest};
use crate::config::{DeploymentTargetConfig, Settings};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use super::super::maintenance_render::{
    render_remote_script, render_service_unit, render_timer_unit, unit_name,
};

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
