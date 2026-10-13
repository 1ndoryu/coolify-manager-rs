/* Split de host_maintenance_manager: checks post-mantenimiento (plan optimizacion seguridad VPS, 1.2).
 * Sin ellos el mantenimiento no se da por completo. */

use super::{contenedores_faltantes, CONTROL_PLANE_REINICIO};
use crate::config::{DeploymentTargetConfig, Settings};
use crate::infra::ssh_client::SshClient;
use crate::services::host_optimization_manager::HostOptimizationRequest;
use crate::services::{control_plane_audit_manager, health_manager, host_optimization_manager};

use serde::Serialize;

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

/// Motivo por el que no se ejecutan los checks; `None` si deben correr.
pub(super) fn motivo_omitir_checks(dry_run: bool, reboot_sin_espera: bool) -> Option<&'static str> {
    if dry_run {
        Some("dry-run: no se aplico mantenimiento, no hay estado nuevo que validar")
    } else if reboot_sin_espera {
        Some("reinicio programado sin --wait: el host aun no ha vuelto; repite con --wait para validarlo")
    } else {
        None
    }
}

pub(super) fn checks_omitidos(motivo: &str) -> Vec<PostMaintenanceCheck> {
    NOMBRES_CHECKS
        .iter()
        .map(|name| PostMaintenanceCheck::new(name, CheckStatus::Skipped, motivo.to_string()))
        .collect()
}

pub(super) async fn ejecutar_post_checks(
    settings: &Settings,
    target: &DeploymentTargetConfig,
    ssh: &SshClient,
) -> Vec<PostMaintenanceCheck> {
    vec![
        check_control_plane(target).await,
        check_sitios_muestra(settings, target, ssh).await,
        check_auditoria(target).await,
        check_optimizacion(target).await,
    ]
}

/// Mismo estado que `coolify-control-plane status` (plan 1.2). `include_proxy` debe ser `true`:
/// `CONTROL_PLANE_REINICIO` incluye `coolify-proxy`, y sin él saldría caído estando sano.
async fn check_control_plane(target: &DeploymentTargetConfig) -> PostMaintenanceCheck {
    match control_plane_audit_manager::status_control_plane_target(target, true).await {
        Ok(pasos) => {
            let activos = pasos
                .iter()
                .find_map(|paso| paso.strip_prefix("running_after="))
                .unwrap_or("none")
                .replace(',', "\n");
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
        Err(error) => {
            PostMaintenanceCheck::new(CHECK_CONTROL_PLANE, CheckStatus::Failed, error.to_string())
        }
    }
}

async fn check_sitios_muestra(
    settings: &Settings,
    target: &DeploymentTargetConfig,
    ssh: &SshClient,
) -> PostMaintenanceCheck {
    let sitios = match settings
        .sample_sites_for_target(&target.name, target.maintenance_policy.as_ref())
    {
        Ok(sitios) => sitios,
        Err(error) => {
            return PostMaintenanceCheck::new(CHECK_SITIOS, CheckStatus::Failed, error.to_string())
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
            format!(
                "{} | {}",
                report.container_summary, report.dominance_summary
            ),
        ),
        Err(error) => {
            PostMaintenanceCheck::new(CHECK_AUDITORIA, CheckStatus::Failed, error.to_string())
        }
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
        Err(error) => {
            PostMaintenanceCheck::new(CHECK_OPTIMIZACION, CheckStatus::Failed, error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
