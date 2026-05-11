/*
 * Comando: restart-site
 * Reinicia los servicios (contenedores) de un sitio via Coolify API.
 *
 * [124A-IMAGE404] Después del restart via Coolify API, Coolify puede reescribir
 * el compose con named volumes. Para sitios con template Rust, se fuerza el
 * bind mount correcto y se reinicia el contenedor app.
 *
 * [115A-STUDIO-AUTOHEAL] Los servicios Rust usan imágenes construidas localmente.
 * Si el Coolify API restart se ejecuta sin imagen disponible, Coolify para el
 * servicio pero no puede levantarlo, dejando el sitio caído indefinidamente.
 * ADEMÁS: restart --all con stacks Rust fue el causante del incidente del
 * 2026-05-11 donde todos los workloads quedaron en exited.
 * SOLUCIÓN: restart --all EXCLUYE sitios Rust por defecto. Para reiniciar un
 * sitio Rust, usar deploy-service que garantiza imagen + bind mounts.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::{site_capabilities, volume_manager};

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: Option<&str>,
    all: bool,
    _only_db: bool,
    _only_wordpress: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;

    let sites_to_restart: Vec<_> = if all {
        let skipped: Vec<_> = settings
            .sitios
            .iter()
            .filter(|s| matches!(s.template, crate::domain::StackTemplate::Rust))
            .map(|s| s.nombre.as_str())
            .collect();
        if !skipped.is_empty() {
            println!(
                "AVISO: restart --all omite sitios Rust ({}). Usan imagen local — usa deploy-service para reconstruirlos.",
                skipped.join(", ")
            );
        }
        settings
            .sitios
            .iter()
            .filter(|s| s.stack_uuid.is_some() && !matches!(s.template, crate::domain::StackTemplate::Rust))
            .collect()
    } else {
        let name = site_name
            .ok_or_else(|| CoolifyError::Validation("Especifica --name o --all".into()))?;
        let site = settings.get_site(name)?;
        validation::assert_site_ready(site)?;
        /* [115A-STUDIO-AUTOHEAL] Rust requiere deploy-service, no restart directo via Coolify API.
         * El API restart sin imagen local deja el servicio en estado exited sin poder levantarlo. */
        if matches!(site.template, crate::domain::StackTemplate::Rust) {
            return Err(CoolifyError::Validation(format!(
                "'{}' es un sitio Rust — usa 'deploy-service --name {}' para reiniciarlo de forma segura (garantiza imagen + bind mounts).",
                name, name
            )));
        }
        vec![site]
    };

    for site in &sites_to_restart {
        let uuid = site.stack_uuid.as_deref().unwrap();
        let target = settings.resolve_site_target(site)?;
        let api = CoolifyApiClient::new(&target.coolify)?;
        tracing::info!("Reiniciando '{}'...", site.nombre);

        match api.restart_service(uuid).await {
            Ok(()) => {
                println!("'{}' reiniciado correctamente.", site.nombre);

                /* [124A-IMAGE404] Coolify puede reescribir compose con named volumes.
                 * Para templates Rust, forzar bind mount y reiniciar con compose correcto. */
                if matches!(site.template, crate::domain::StackTemplate::Rust) {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    let mut ssh = SshClient::from_vps(&target.vps);
                    ssh.connect().await?;
                    let service_dir = format!("/data/coolify/services/{}", uuid);
                    volume_manager::ensure_uploads_host_dir(&ssh, &site.nombre).await?;
                    volume_manager::ensure_uploads_bind_mount(&ssh, &service_dir, &site.nombre)
                        .await?;
                    let caps = site_capabilities::resolve(site);
                    let compose_up = ssh
                        .execute(&format!(
                            "cd {} && docker compose up -d --no-build {} 2>&1",
                            service_dir, caps.app_name_hint
                        ))
                        .await?;
                    if !compose_up.success() {
                        return Err(CoolifyError::Validation(format!(
                            "Restart local de '{}' fallo: {}{}",
                            site.nombre,
                            compose_up.stdout.trim(),
                            compose_up.stderr.trim()
                        )));
                    }
                    volume_manager::verify_runtime_uploads_bind_mount(
                        &ssh,
                        &service_dir,
                        caps.app_name_hint,
                        &site.nombre,
                    )
                    .await?;
                }
            }
            Err(e) => {
                tracing::error!("Error reiniciando '{}': {e}", site.nombre);
                println!("Error reiniciando '{}': {e}", site.nombre);
            }
        }
    }

    Ok(())
}
