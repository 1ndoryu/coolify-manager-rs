/*
 * Comando: redeploy
 * Fuerza un redeploy del servicio via Coolify API con health check posterior.
 *
 * [124A-IMAGE404] Después del stop+start de Coolify, Coolify reescribe el
 * compose en disco con named volumes (normaliza bind mounts). Este comando
 * ahora fuerza el bind mount correcto después del start y reinicia el
 * contenedor app para que use el compose corregido.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::{health_manager, site_capabilities, volume_manager};

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

    /* Esperar a que Coolify escriba compose y arranque contenedores */
    tokio::time::sleep(std::time::Duration::from_secs(15)).await;

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    /* [124A-IMAGE404] Coolify acaba de reescribir el compose con named volumes.
     * Forzar bind mount y reiniciar el contenedor app con el compose corregido.
     * Sin esto, las imágenes subidas se pierden porque Docker usa un named volume vacío. */
    let service_dir = format!("/data/coolify/services/{}", stack_uuid);
    volume_manager::ensure_uploads_host_dir(&ssh, &site.nombre).await?;
    volume_manager::ensure_uploads_bind_mount(&ssh, &service_dir, &site.nombre).await?;

    let caps = site_capabilities::resolve(site);
    let restart_cmd = format!(
        "cd {} && docker compose up -d --no-build {} 2>&1",
        service_dir, caps.app_name_hint
    );
    ssh.execute(&restart_cmd).await?;
    println!("Contenedor reiniciado con bind mount corregido.");

    /* Esperar a que el contenedor arranque */
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

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
