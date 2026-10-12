use crate::config::{DeploymentTargetConfig, Settings, VpsConfig};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::services::host_optimization_manager::HostOptimizationRequest;
use crate::services::{control_plane_audit_manager, health_manager, host_optimization_manager};

use serde::Serialize;
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Plazo total para que el host vuelva tras el reboot (mismo tope que el lock de apt).
const ESPERA_REINICIO_SECS: u64 = 900;
/// El reboot se programa con 3 s de retraso: sin esta pausa el sondeo vería el host aún arriba.
const ESPERA_INICIAL_SECS: u64 = 20;
const INTERVALO_SONDEO_SECS: u64 = 15;
const BOOT_ID_COMANDO: &str = "cat /proc/sys/kernel/random/boot_id";
const COMANDO_CONTENEDORES: &str = "bash -lc 'docker ps --format \"{{.Names}}\"'";
/// Subconjunto de CONTROL_PLANE_CONTAINERS (control_plane_audit_manager/tipos.rs, privado a su módulo).
const CONTROL_PLANE_REINICIO: &[&str] = &["coolify", "coolify-db", "coolify-redis", "coolify-proxy"];
/// Checks post-mantenimiento (plan optimizacion seguridad VPS, 1.2): sin ellos el mantenimiento no se da por completo.
const CHECK_CONTROL_PLANE: &str = "control-plane";
const CHECK_SITIOS: &str = "sitios-muestra";
const CHECK_AUDITORIA: &str = "audit-control-plane";
const CHECK_OPTIMIZACION: &str = "optimize-host-dry-run";
const NOMBRES_CHECKS: &[&str] = &[
    CHECK_CONTROL_PLANE,
    CHECK_SITIOS,
    CHECK_AUDITORIA,
    CHECK_OPTIMIZACION,
];
const CHECK_AUDITORIA_SINCE: &str = "15m";
/// El check solo mide (dry-run). `optimize-host` usa 1 muestra por defecto; aquí 3 para promediar CPU.
const CHECK_OPTIMIZACION_MUESTRAS: u8 = 3;
const CHECK_OPTIMIZACION_INTERVALO_SECS: u8 = 5;

#[derive(Debug, Clone)]
pub struct HostMaintenanceRequest {
    pub reboot: bool,
    pub wait: bool,
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
    pub post_checks: Vec<PostMaintenanceCheck>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Passed,
    Failed,
    Skipped,
}

impl CheckStatus {
    pub fn etiqueta(self) -> &'static str {
        match self {
            CheckStatus::Passed => "ok",
            CheckStatus::Failed => "fallo",
            CheckStatus::Skipped => "omitido",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PostMaintenanceCheck {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
}

impl PostMaintenanceCheck {
    fn new(name: &str, status: CheckStatus, detail: String) -> Self {
        Self {
            name: name.to_string(),
            status,
            detail,
        }
    }
}

pub async fn maintain_default_vps(
    settings: &Settings,
    request: &HostMaintenanceRequest,
) -> std::result::Result<HostMaintenanceReport, CoolifyError> {
    let target = settings.default_target();
    maintain_vps_config(settings, &target, request).await
}

pub async fn maintain_target(
    settings: &Settings,
    target: &DeploymentTargetConfig,
    request: &HostMaintenanceRequest,
) -> std::result::Result<HostMaintenanceReport, CoolifyError> {
    maintain_vps_config(settings, target, request).await
}

async fn maintain_vps_config(
    settings: &Settings,
    target: &DeploymentTargetConfig,
    request: &HostMaintenanceRequest,
) -> std::result::Result<HostMaintenanceReport, CoolifyError> {
    let target_name = target.name.as_str();
    let vps = &target.vps;
    let mut ssh = SshClient::from_vps(vps);
    ssh.connect().await?;

    let os_name = exec_trim(
        &ssh,
        "sh -lc '. /etc/os-release 2>/dev/null; echo ${PRETTY_NAME:-unknown}'",
    )
    .await?;
    let mut running_kernel = exec_trim(&ssh, "uname -r").await?;

    let mut applied_steps = Vec::new();
    let package_summary = if request.dry_run {
        applied_steps.push("Dry run: se ejecutaria apt-get update && apt-get -y full-upgrade && apt-get -y autoremove.".to_string());
        "dry-run".to_string()
    } else {
        run_package_maintenance(&mut ssh, target_name).await?
    };

    let mut installed_kernel = detect_installed_kernel(&ssh).await?;
    let mut pendiente_reinicio = reboot_required(&ssh, &running_kernel, &installed_kernel).await?;

    let reboot_scheduled = if request.reboot {
        if request.dry_run {
            applied_steps.push(
                "Dry run: se programaria reboot del host 3s despues de terminar la actualizacion."
                    .to_string(),
            );
            if request.wait {
                applied_steps.push(
                    "Dry run: se esperaria a que el host vuelva y se validaria kernel y control-plane."
                        .to_string(),
                );
            }
            false
        } else {
            let boot_previo = exec_trim(&ssh, BOOT_ID_COMANDO).await?;
            schedule_reboot(&ssh).await?;
            applied_steps.push(
                "Reboot programado en background con retraso corto para liberar la sesion SSH."
                    .to_string(),
            );
            if request.wait {
                let limite = Instant::now() + Duration::from_secs(ESPERA_REINICIO_SECS);
                ssh = esperar_reinicio(vps, &boot_previo, limite).await?;
                esperar_control_plane(&ssh, limite).await?;
                (running_kernel, installed_kernel, pendiente_reinicio) =
                    estado_kernel(&ssh).await?;
                if pendiente_reinicio {
                    return Err(CoolifyError::Validation(format!(
                        "Tras el reinicio sigue pendiente un reboot (kernel en ejecucion {running_kernel}, instalado {installed_kernel}); revisa el host antes de repetir"
                    )));
                }
                applied_steps.push(
                    "Host de vuelta tras el reinicio: kernel al dia y contenedores del control-plane arriba."
                        .to_string(),
                );
            }
            true
        }
    } else {
        false
    };

    if !request.dry_run {
        applied_steps
            .push("Mantenimiento de paquetes completado via coolify-manager-rs.".to_string());
    }

    let recommendations =
        build_recommendations(pendiente_reinicio, request.reboot, &package_summary);

    let post_checks = match motivo_omitir_checks(request.dry_run, reboot_scheduled && !request.wait)
    {
        Some(motivo) => checks_omitidos(motivo),
        None => ejecutar_post_checks(settings, target, &ssh).await,
    };

    Ok(HostMaintenanceReport {
        target: target_name.to_string(),
        os_name: empty_as_unknown(&os_name),
        package_summary: empty_as_unknown(&package_summary),
        running_kernel: empty_as_unknown(&running_kernel),
        installed_kernel: empty_as_unknown(&installed_kernel),
        reboot_required: pendiente_reinicio,
        reboot_scheduled,
        applied_steps,
        recommendations,
        post_checks,
    })
}

/// Motivo por el que no se ejecutan los checks; `None` si deben correr.
fn motivo_omitir_checks(dry_run: bool, reboot_sin_espera: bool) -> Option<&'static str> {
    if dry_run {
        Some("dry-run: no se aplico mantenimiento, no hay estado nuevo que validar")
    } else if reboot_sin_espera {
        Some("reinicio programado sin --wait: el host aun no ha vuelto; repite con --wait para validarlo")
    } else {
        None
    }
}

fn checks_omitidos(motivo: &str) -> Vec<PostMaintenanceCheck> {
    NOMBRES_CHECKS
        .iter()
        .map(|name| PostMaintenanceCheck::new(name, CheckStatus::Skipped, motivo.to_string()))
        .collect()
}

async fn ejecutar_post_checks(
    settings: &Settings,
    target: &DeploymentTargetConfig,
    ssh: &SshClient,
) -> Vec<PostMaintenanceCheck> {
    vec![
        check_control_plane(ssh).await,
        check_sitios_muestra(settings, target, ssh).await,
        check_auditoria(target).await,
        check_optimizacion(target).await,
    ]
}

async fn check_control_plane(ssh: &SshClient) -> PostMaintenanceCheck {
    match exec_trim(ssh, COMANDO_CONTENEDORES).await {
        Ok(activos) => {
            let faltantes = contenedores_faltantes(CONTROL_PLANE_REINICIO, &activos);
            if faltantes.is_empty() {
                PostMaintenanceCheck::new(
                    CHECK_CONTROL_PLANE,
                    CheckStatus::Passed,
                    "contenedores del control-plane arriba".to_string(),
                )
            } else {
                PostMaintenanceCheck::new(
                    CHECK_CONTROL_PLANE,
                    CheckStatus::Failed,
                    format!("sin arrancar: {}", faltantes.join(", ")),
                )
            }
        }
        Err(error) => PostMaintenanceCheck::new(
            CHECK_CONTROL_PLANE,
            CheckStatus::Failed,
            error.to_string(),
        ),
    }
}

async fn check_sitios_muestra(
    settings: &Settings,
    target: &DeploymentTargetConfig,
    ssh: &SshClient,
) -> PostMaintenanceCheck {
    let sitios =
        match settings.sample_sites_for_target(&target.name, target.maintenance_policy.as_ref()) {
            Ok(sitios) => sitios,
            Err(error) => {
                return PostMaintenanceCheck::new(
                    CHECK_SITIOS,
                    CheckStatus::Failed,
                    error.to_string(),
                )
            }
        };
    if sitios.is_empty() {
        return PostMaintenanceCheck::new(
            CHECK_SITIOS,
            CheckStatus::Skipped,
            "el target no tiene sitios de muestra".to_string(),
        );
    }

    let mut fallos = Vec::new();
    for site in &sitios {
        match health_manager::run_site_health_check(settings, site, ssh).await {
            Ok(report) if report.healthy() => {}
            Ok(report) => fallos.push(format!("{}: {}", site.nombre, report.details.join("; "))),
            Err(error) => fallos.push(format!("{}: {}", site.nombre, error)),
        }
    }
    if fallos.is_empty() {
        PostMaintenanceCheck::new(
            CHECK_SITIOS,
            CheckStatus::Passed,
            format!("{} sitios sanos", sitios.len()),
        )
    } else {
        PostMaintenanceCheck::new(CHECK_SITIOS, CheckStatus::Failed, fallos.join(" | "))
    }
}

async fn check_auditoria(target: &DeploymentTargetConfig) -> PostMaintenanceCheck {
    match control_plane_audit_manager::audit_target(target, CHECK_AUDITORIA_SINCE).await {
        Ok(report) => PostMaintenanceCheck::new(
            CHECK_AUDITORIA,
            CheckStatus::Passed,
            format!("{} | {}", report.container_summary, report.dominance_summary),
        ),
        Err(error) => PostMaintenanceCheck::new(
            CHECK_AUDITORIA,
            CheckStatus::Failed,
            error.to_string(),
        ),
    }
}

async fn check_optimizacion(target: &DeploymentTargetConfig) -> PostMaintenanceCheck {
    let request = HostOptimizationRequest {
        swap_gb: 4,
        swappiness: 10,
        vfs_cache_pressure: 50,
        overcommit_memory: 1,
        disable_thp: false,
        docker_live_restore: false,
        dry_run: true,
        samples: CHECK_OPTIMIZACION_MUESTRAS,
        interval_seconds: CHECK_OPTIMIZACION_INTERVALO_SECS,
    };
    match host_optimization_manager::optimize_target(target, &request).await {
        Ok(report) => PostMaintenanceCheck::new(
            CHECK_OPTIMIZACION,
            CheckStatus::Passed,
            report.sampling_summary,
        ),
        Err(error) => PostMaintenanceCheck::new(
            CHECK_OPTIMIZACION,
            CheckStatus::Failed,
            error.to_string(),
        ),
    }
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

/// Estado del kernel tras el reinicio: (kernel en ejecucion, kernel instalado, reboot pendiente).
async fn estado_kernel(
    ssh: &SshClient,
) -> std::result::Result<(String, String, bool), CoolifyError> {
    let running = exec_trim(ssh, "uname -r").await?;
    let installed = detect_installed_kernel(ssh).await?;
    let pendiente = reboot_required(ssh, &running, &installed).await?;
    Ok((running, installed, pendiente))
}

/// Sondea hasta que el host responda con un boot_id distinto del previo.
async fn esperar_reinicio(
    vps: &VpsConfig,
    boot_previo: &str,
    limite: Instant,
) -> std::result::Result<SshClient, CoolifyError> {
    tokio::time::sleep(Duration::from_secs(ESPERA_INICIAL_SECS)).await;
    loop {
        let motivo = match conectar_y_leer_boot_id(vps).await {
            Ok((ssh, boot)) if reinicio_completado(boot_previo, &boot) => return Ok(ssh),
            Ok(_) => "el host sigue con el arranque anterior".to_string(),
            Err(error) => error.to_string(),
        };
        if Instant::now() >= limite {
            return Err(CoolifyError::Validation(format!(
                "El host no volvio tras el reinicio en {} s: {}",
                ESPERA_REINICIO_SECS, motivo
            )));
        }
        tokio::time::sleep(Duration::from_secs(INTERVALO_SONDEO_SECS)).await;
    }
}

async fn conectar_y_leer_boot_id(
    vps: &VpsConfig,
) -> std::result::Result<(SshClient, String), CoolifyError> {
    let mut ssh = SshClient::from_vps(vps);
    ssh.connect().await?;
    let boot = exec_trim(&ssh, BOOT_ID_COMANDO).await?;
    Ok((ssh, boot))
}

/// Espera a que los contenedores del plano de control arranquen (Docker tarda tras el reboot).
async fn esperar_control_plane(
    ssh: &SshClient,
    limite: Instant,
) -> std::result::Result<(), CoolifyError> {
    let mut ultimo_error = String::from("ninguno");
    loop {
        let activos = match exec_trim(ssh, COMANDO_CONTENEDORES).await {
            Ok(salida) => salida,
            Err(error) => {
                ultimo_error = error.to_string();
                String::new()
            }
        };
        let faltantes = contenedores_faltantes(CONTROL_PLANE_REINICIO, &activos);
        if faltantes.is_empty() {
            return Ok(());
        }
        if Instant::now() >= limite {
            return Err(CoolifyError::Validation(format!(
                "Control-plane incompleto tras el reinicio, sin arrancar: {} (ultimo error de docker ps: {})",
                faltantes.join(", "),
                ultimo_error
            )));
        }
        tokio::time::sleep(Duration::from_secs(INTERVALO_SONDEO_SECS)).await;
    }
}

/// El reinicio ocurrio si el boot_id leido ahora no esta vacio y difiere del previo.
fn reinicio_completado(boot_previo: &str, boot_actual: &str) -> bool {
    !boot_actual.is_empty() && boot_actual != boot_previo
}

/// Contenedores requeridos ausentes en la salida de `docker ps --format {{.Names}}`.
fn contenedores_faltantes(requeridos: &[&str], activos: &str) -> Vec<String> {
    let presentes: HashSet<&str> = activos.lines().map(str::trim).collect();
    requeridos
        .iter()
        .filter(|nombre| !presentes.contains(*nombre))
        .map(|nombre| nombre.to_string())
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reinicio_completado_exige_boot_id_distinto_y_no_vacio() {
        assert!(reinicio_completado("a1", "b2"));
        assert!(!reinicio_completado("a1", "a1"));
        assert!(!reinicio_completado("a1", ""));
    }

    #[test]
    fn contenedores_faltantes_lista_solo_los_no_activos() {
        assert_eq!(
            contenedores_faltantes(CONTROL_PLANE_REINICIO, "coolify\ncoolify-db\n"),
            ["coolify-redis", "coolify-proxy"]
        );
        assert!(contenedores_faltantes(
            CONTROL_PLANE_REINICIO,
            "coolify\ncoolify-db\ncoolify-redis\ncoolify-proxy\notro\n"
        )
        .is_empty());
    }

    #[test]
    fn checks_se_omiten_en_dry_run_y_reboot_sin_espera() {
        assert!(motivo_omitir_checks(true, false).is_some());
        assert!(motivo_omitir_checks(false, true).is_some());
        assert!(motivo_omitir_checks(false, false).is_none());
        let omitidos = checks_omitidos("dry-run");
        assert_eq!(omitidos.len(), NOMBRES_CHECKS.len());
        assert!(omitidos
            .iter()
            .all(|check| check.status == CheckStatus::Skipped));
    }
}
