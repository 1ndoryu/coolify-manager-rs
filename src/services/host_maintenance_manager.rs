use crate::config::{DeploymentTargetConfig, Settings, VpsConfig};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use serde::Serialize;

#[derive(Debug, Clone)]
pub struct HostMaintenanceRequest {
    pub reboot: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostMaintenanceReport {
    pub target: String,
    pub os_name: String,
    pub package_summary: String,
    pub running_kernel: String,
    pub installed_kernel: String,
    pub reboot_required: bool,
    pub reboot_scheduled: bool,
    pub applied_steps: Vec<String>,
    pub recommendations: Vec<String>,
}

pub async fn maintain_default_vps(
    settings: &Settings,
    request: &HostMaintenanceRequest,
) -> std::result::Result<HostMaintenanceReport, CoolifyError> {
    maintain_vps_config("default", &settings.vps, request).await
}

pub async fn maintain_target(
    target: &DeploymentTargetConfig,
    request: &HostMaintenanceRequest,
) -> std::result::Result<HostMaintenanceReport, CoolifyError> {
    maintain_vps_config(&target.name, &target.vps, request).await
}

async fn maintain_vps_config(
    target_name: &str,
    vps: &VpsConfig,
    request: &HostMaintenanceRequest,
) -> std::result::Result<HostMaintenanceReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(vps);
    ssh.connect().await?;

    let os_name = exec_trim(
        &ssh,
        "sh -lc '. /etc/os-release 2>/dev/null; echo ${PRETTY_NAME:-unknown}'",
    )
    .await?;
    let running_kernel = exec_trim(&ssh, "uname -r").await?;

    let mut applied_steps = Vec::new();
    let package_summary = if request.dry_run {
        applied_steps.push("Dry run: se ejecutaria apt-get update && apt-get -y full-upgrade && apt-get -y autoremove.".to_string());
        "dry-run".to_string()
    } else {
        run_package_maintenance(&mut ssh, target_name).await?
    };

    let installed_kernel = detect_installed_kernel(&ssh).await?;
    let reboot_required = reboot_required(&ssh, &running_kernel, &installed_kernel).await?;

    let reboot_scheduled = if request.reboot {
        if request.dry_run {
            applied_steps.push("Dry run: se programaria reboot del host 3s despues de terminar la actualizacion.".to_string());
            false
        } else {
            schedule_reboot(&ssh).await?;
            applied_steps.push("Reboot programado en background con retraso corto para liberar la sesion SSH.".to_string());
            true
        }
    } else {
        false
    };

    if !request.dry_run {
        applied_steps.push("Mantenimiento de paquetes completado via coolify-manager-rs.".to_string());
    }

    let recommendations = build_recommendations(reboot_required, request.reboot, &package_summary);

    Ok(HostMaintenanceReport {
        target: target_name.to_string(),
        os_name: empty_as_unknown(&os_name),
        package_summary: empty_as_unknown(&package_summary),
        running_kernel: empty_as_unknown(&running_kernel),
        installed_kernel: empty_as_unknown(&installed_kernel),
        reboot_required,
        reboot_scheduled,
        applied_steps,
        recommendations,
    })
}

async fn run_package_maintenance(
    ssh: &mut SshClient,
    target_name: &str,
) -> std::result::Result<String, CoolifyError> {
        let script = r#"set -e
export DEBIAN_FRONTEND=noninteractive
lock_wait=0
while true; do
    lock_pid=""
    if command -v fuser >/dev/null 2>&1; then
        lock_pid=$(fuser /var/lib/dpkg/lock-frontend 2>/dev/null | awk 'NR==1 {print $1}')
    fi
    if [ -z "$lock_pid" ]; then
        lock_pid=$(pgrep -x apt-get 2>/dev/null | head -1 || true)
    fi
    if [ -z "$lock_pid" ]; then
        break
    fi
    if [ "$lock_wait" -ge 900 ]; then
        cmd=$(tr '\0' ' ' < /proc/$lock_pid/cmdline 2>/dev/null || echo unknown)
        echo APT_LOCK_TIMEOUT pid=$lock_pid cmd=$cmd waited=${lock_wait}s
        exit 99
    fi
    cmd=$(tr '\0' ' ' < /proc/$lock_pid/cmdline 2>/dev/null || echo unknown)
    echo APT_LOCK_WAIT pid=$lock_pid cmd=$cmd waited=${lock_wait}s
    sleep 15
    lock_wait=$((lock_wait + 15))
done
if dpkg --audit 2>/dev/null | grep -q .; then
    echo DPKG_RECOVERY_START
    dpkg --configure -a
fi
apt-get update
apt-get -y full-upgrade
apt-get -y autoremove --purge
apt-get clean
updates=$(apt list --upgradable 2>/dev/null | sed '1d' | wc -l)
printf 'remaining_upgradable=%s\n' "$updates"
"#;
    let command = format!("bash -lc {}", sh_quote(script));
    let safe_target = target_name.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
    let log_file = format!("/tmp/cm-maintain-host-{}.log", safe_target);
    let result = ssh
        .execute_long_running(&command, &log_file, 10, 3600)
        .await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo actualizando paquetes del host: {}{}",
            result.stdout, result.stderr
        )));
    }

    let summary = result
        .stdout
        .lines()
        .rev()
        .find(|line| line.contains("remaining_upgradable="))
        .map(str::trim)
        .unwrap_or("remaining_upgradable=unknown");
    Ok(summary.to_string())
}

async fn detect_installed_kernel(ssh: &SshClient) -> std::result::Result<String, CoolifyError> {
    let command = r#"bash -lc 'dpkg-query -W -f=${Version} linux-image-generic 2>/dev/null || dpkg-query -W -f=${Version} linux-image-generic-hwe-24.04 2>/dev/null || uname -r'"#;
    exec_trim(ssh, command).await
}

async fn reboot_required(
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

async fn schedule_reboot(ssh: &SshClient) -> std::result::Result<(), CoolifyError> {
    let command = r#"bash -lc 'nohup sh -c "sleep 3; systemctl reboot" >/dev/null 2>&1 & echo REBOOT_SCHEDULED'"#;
    let result = ssh.execute(command).await?;
    if !result.success() || !result.stdout.contains("REBOOT_SCHEDULED") {
        return Err(CoolifyError::Validation(format!(
            "No se pudo programar el reboot del host: {}{}",
            result.stdout, result.stderr
        )));
    }
    Ok(())
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

fn build_recommendations(
    reboot_required: bool,
    reboot_requested: bool,
    package_summary: &str,
) -> Vec<String> {
    let mut notes = Vec::new();
    if reboot_required && !reboot_requested {
        notes.push("El host quedo con reboot pendiente; ejecuta maintain-host --reboot para cargar kernel/servicios nuevos.".to_string());
    }
    if package_summary == "dry-run" {
        notes.push(
            "Dry run: no se inspecciono el numero real de paquetes pendientes; usa maintain-host sin --dry-run para obtener el estado exacto."
                .to_string(),
        );
    } else if package_summary.contains("remaining_upgradable=0") {
        notes.push("No quedan paquetes upgradable; la siguiente mejora ya no es parcheo sino tuning de carga, control-plane y aislamiento de workloads.".to_string());
    } else {
        notes.push("Quedaron paquetes pendientes; revisa held packages o repositorios de terceros antes de repetir el mantenimiento.".to_string());
    }
    if reboot_requested {
        notes.push("Tras el reboot, valida Coolify control-plane y health HTTP de los sitios antes de seguir optimizando.".to_string());
    }
    notes
}

fn empty_as_unknown(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}