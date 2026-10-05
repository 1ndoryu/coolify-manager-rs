/*
 * Comando: stop
 * Detiene los servicios (contenedores) de UN sitio via Coolify API, sin
 * borrar nada: la imagen y los volúmenes quedan intactos en el VPS.
 *
 * Sin flag --all a propósito: parar todos los sitios de golpe es el gemelo
 * peligroso de `restart --all` (lección 2026-05-11). Un sitio por invocación.
 *
 * Nota: no existe comando `start` en el CLI; para rearrancar, botón Start en
 * el panel de Coolify o `deploy-service --name <sitio>` (Rust).
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::validation;

use std::path::Path;

pub async fn execute(config_path: &Path, site_name: &str) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    let uuid = site.stack_uuid.as_deref().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{site_name}' no tiene stack_uuid"))
    })?;
    let target = settings.resolve_site_target(site)?;
    let api = CoolifyApiClient::new(&target.coolify)?;
    tracing::info!("Deteniendo '{site_name}'...");
    api.stop_service(uuid).await?;
    println!("'{site_name}' detenido (contenedores parados; imagen y datos intactos).");
    println!("Rearranque: botón Start en Coolify o 'deploy-service --name {site_name}' (Rust).");
    Ok(())
}
