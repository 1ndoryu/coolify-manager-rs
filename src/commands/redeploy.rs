/*
 * Comando: redeploy
 * Fuerza un redeploy del servicio via Coolify API con health check posterior.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::health_manager;

use std::path::Path;

pub async fn execute(config_path: &Path, site_name: &str) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    let stack_uuid = site.stack_uuid.as_deref().unwrap();
    let target = settings.resolve_site_target(site)?;

    let api = CoolifyApiClient::new(&target.coolify)?;

    tracing::info!("Forzando redeploy de '{site_name}' (uuid: {stack_uuid})");

    /* Stop + Start = redeploy completo (rebuild containers) */
    api.stop_service(stack_uuid).await?;

    tracing::info!("Servicio detenido, esperando antes de reiniciar...");
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    api.start_service(stack_uuid).await?;

    println!("Redeploy iniciado para '{site_name}'. Esperando estabilizacion...");

    /* Esperar a que los contenedores arranquen antes de verificar salud */
    tokio::time::sleep(std::time::Duration::from_secs(15)).await;

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let report = health_manager::assert_site_healthy(&settings, site, &ssh).await?;

    if report.healthy() {
        println!("Health check: OK — redeploy exitoso.");
    } else {
        for detail in &report.details {
            println!("  - {detail}");
        }
        return Err(CoolifyError::Validation(format!(
            "Redeploy completado pero el sitio '{}' no paso health check",
            site_name
        )));
    }

    Ok(())
}
