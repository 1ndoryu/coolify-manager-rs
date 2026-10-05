/*
 * Comando: set-build-mode [01AA-3]
 * Cambia dónde compila deploy-service para un sitio: laptop | vps.
 * Solo toca settings.json (sin SSH, sin Coolify): el próximo
 * deploy-service ya usa el modo nuevo. Rollback = volver a `vps`.
 */

use crate::config::Settings;
use crate::domain::BuildMode;
use crate::error::CoolifyError;
use crate::infra::validation;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    mode: &str,
) -> std::result::Result<(), CoolifyError> {
    let nuevo = BuildMode::parse(mode)?;

    let mut settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;
    let anterior = site.build_mode;
    let template = site.template.clone();

    if anterior == nuevo {
        println!("'{site_name}' ya está en build_mode={nuevo} (sin cambios).");
        return Ok(());
    }
    if let Some(sitio) = settings.sitios.iter_mut().find(|s| s.nombre == site_name) {
        sitio.build_mode = nuevo;
    }
    settings.save(config_path)?;

    println!("build_mode de '{site_name}': {anterior} -> {nuevo}");
    if nuevo == BuildMode::Vps {
        println!(
            "Nota: volver a vps cuesta un build clásico en el VPS en el próximo deploy-service."
        );
    }
    if nuevo == BuildMode::Laptop && !BuildMode::tiene_build_local(&template) {
        println!(
            "Aviso: template {template} sin build local: deploy-service seguirá la ruta clásica hasta que el template la soporte."
        );
    }
    Ok(())
}
