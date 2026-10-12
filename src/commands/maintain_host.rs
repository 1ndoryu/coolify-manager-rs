use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::host_maintenance_manager::{
    self, check_target, HostCheckReport, HostMaintenanceRequest,
};

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    target_name: Option<&str>,
    reboot: bool,
    wait: bool,
    dry_run: bool,
    check: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    if check {
        return ejecutar_chequeo(&settings, target_name).await;
    }

    let request = HostMaintenanceRequest {
        reboot,
        wait,
        dry_run,
    };

    let report = match target_name {
        Some(name) => {
            let target = settings.get_target(name)?;
            host_maintenance_manager::maintain_target(&settings, target, &request).await?
        }
        None => host_maintenance_manager::maintain_default_vps(&settings, &request).await?,
    };

    println!("Target: {}", report.target);
    println!("SO: {}", report.os_name);
    println!("Estado paquetes: {}", report.package_summary);
    println!(
        "Paquetes instalados o actualizados: {}",
        contar(report.packages_changed)
    );
    println!(
        "Paquetes pendientes: {}",
        contar(report.remaining_upgradable)
    );
    println!("Kernel previo: {}", report.previous_kernel);
    println!("Kernel running: {}", report.running_kernel);
    println!("Kernel instalado: {}", report.installed_kernel);
    println!(
        "Reinicio requerido: {}",
        if report.reboot_required { "si" } else { "no" }
    );
    println!(
        "Reinicio programado: {}",
        if report.reboot_scheduled { "si" } else { "no" }
    );
    println!(
        "Reinicio completado: {}",
        if report.reboot_completed { "si" } else { "no" }
    );
    println!("Duracion total (s): {}", report.duration_secs);
    for step in report.applied_steps {
        println!("- {step}");
    }
    for recommendation in report.recommendations {
        println!("- {recommendation}");
    }

    println!("Checks post-mantenimiento:");
    for check in &report.post_checks {
        println!(
            "- [{}] {}: {}",
            check.status.etiqueta(),
            check.name,
            check.detail
        );
    }
    let fallidos: Vec<&str> = report
        .post_checks
        .iter()
        .filter(|check| check.status == host_maintenance_manager::CheckStatus::Failed)
        .map(|check| check.name.as_str())
        .collect();
    if !fallidos.is_empty() {
        return Err(CoolifyError::Validation(format!(
            "Checks post-mantenimiento fallidos: {}",
            fallidos.join(", ")
        )));
    }

    Ok(())
}

/// Solo lectura: no altera paquetes ni reinicia. Informativo, sale con 0 aunque haya pendientes.
async fn ejecutar_chequeo(
    settings: &Settings,
    target_name: Option<&str>,
) -> std::result::Result<(), CoolifyError> {
    let reporte = match target_name {
        Some(name) => check_target(settings.get_target(name)?).await?,
        None => check_target(&settings.default_target()).await?,
    };
    imprimir_chequeo(&reporte);
    Ok(())
}

fn imprimir_chequeo(reporte: &HostCheckReport) {
    println!("Target: {}", reporte.target);
    println!("SO: {}", reporte.os_name);
    println!("Kernel running: {}", reporte.running_kernel);
    println!("Kernel instalado: {}", reporte.installed_kernel);
    println!(
        "Reinicio requerido: {}",
        if reporte.reboot_required { "si" } else { "no" }
    );
    println!(
        "Indice apt actualizado: {}",
        if reporte.apt_index_updated {
            "si"
        } else {
            "no"
        }
    );
    println!(
        "Paquetes actualizables: {}",
        contar(reporte.upgradable_packages)
    );
    println!(
        "Paquetes retenidos: {}",
        lista_o_ninguno(&reporte.held_packages)
    );
    println!(
        "Servicios fallidos: {}",
        lista_o_ninguno(&reporte.failed_units)
    );
    println!(
        "Antiguedad aviso de reinicio: {}",
        reporte.reboot_pending_days.map_or_else(
            || "n/d (sin aviso de dpkg)".to_string(),
            |d| format!("{d} dias")
        )
    );
    println!(
        "Solapes con el manager: {}",
        lista_o_ninguno(&reporte.overlaps)
    );
}

fn contar(valor: Option<u64>) -> String {
    valor.map_or_else(|| "unknown".to_string(), |n| n.to_string())
}

fn lista_o_ninguno(items: &[String]) -> String {
    if items.is_empty() {
        "ninguno".to_string()
    } else {
        items.join(", ")
    }
}
