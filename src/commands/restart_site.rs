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
use crate::services::{restart_policy, site_capabilities, volume_manager};

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
        /* [01AA-1] Los stacks con imagen cm-local (build-laptop) tampoco
         * sobreviven al restart vía API: Coolify intenta rebuild/pull y el
         * servicio queda en Created sin imagen. Se omiten igual que Rust. */
        let skipped_img: Vec<_> = settings
            .sitios
            .iter()
            .filter(|s| restart_policy::usa_imagen_local_cm(s.image_ref.as_deref()))
            .map(|s| s.nombre.as_str())
            .collect();
        if !skipped_img.is_empty() {
            println!(
                "AVISO: restart --all omite sitios con imagen local cm-local ({}). Reconstruye con build-laptop + new/deploy-service --image.",
                skipped_img.join(", ")
            );
        }
        settings
            .sitios
            .iter()
            .filter(|s| {
                s.stack_uuid.is_some()
                    && !matches!(s.template, crate::domain::StackTemplate::Rust)
                    && !restart_policy::usa_imagen_local_cm(s.image_ref.as_deref())
            })
            .collect()
    } else {
        let name = site_name
            .ok_or_else(|| CoolifyError::Validation("Especifica --name o --all".into()))?;
        let site = settings.get_site(name)?;
        validation::assert_site_ready(site)?;
        /* [310A-1] Un solo guarda para Rust y para imagen cm-local: el restart
         * vía API para el servicio y no puede recrearlo. Vivia duplicado aqui
         * y env_toggle --restart lo esquivaba. */
        if let Some(motivo) = restart_policy::motivo_no_reiniciable_api(site) {
            return Err(CoolifyError::Validation(motivo));
        }
        vec![site]
    };

    for site in &sites_to_restart {
        let uuid = site.stack_uuid.as_deref().ok_or_else(|| {
            CoolifyError::Validation(format!("Sitio '{}' no tiene stack_uuid", site.nombre))
        })?;
        let target = settings.resolve_site_target(site)?;
        let api = CoolifyApiClient::new(&target.coolify)?;
        tracing::info!("Reiniciando '{}'...", site.nombre);

        match api.restart_service(uuid).await {
            Ok(()) => {
                /* [310A-1] La API responde 202 en cuanto Coolify acepta la orden; el
                 * restart real corre despues y puede fallar (2026-10-18: CoolifyTask
                 * FAIL 4 s despues y el servicio quedo sin contenedores). No se
                 * afirma exito: se dice lo que se sabe y como confirmarlo. */
                println!(
                    "'{}': orden de reinicio aceptada por Coolify (corre en segundo plano). Confirma con 'health --name {}'.",
                    site.nombre, site.nombre
                );

                /* [124A-IMAGE404] Coolify puede reescribir compose con named volumes.
                 * Para templates Rust, forzar bind mount y reiniciar con compose correcto. */
                if matches!(site.template, crate::domain::StackTemplate::Rust) {
                    forzar_bind_mount_rust(&target.vps, site, uuid).await?;
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

/* [06AA-4] Post-restart de sitios Rust ([124A-IMAGE404]): Coolify puede
 * reescribir el compose con named volumes; se fuerza el bind mount correcto
 * y se reinicia el contenedor app. Extraído de execute()
 * (funcion-larga-rs); sin cambios de comportamiento. */
async fn forzar_bind_mount_rust(
    vps: &crate::config::VpsConfig,
    site: &crate::domain::SiteConfig,
    uuid: &str,
) -> std::result::Result<(), CoolifyError> {
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    let mut ssh = SshClient::from_vps(vps);
    ssh.connect().await?;
    let service_dir = format!("/data/coolify/services/{}", uuid);
    volume_manager::ensure_uploads_host_dir(&ssh, &site.nombre).await?;
    let caps = site_capabilities::resolve(site);
    volume_manager::ensure_uploads_bind_mount(&ssh, &service_dir, &site.nombre, caps.app_name_hint)
        .await?;
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
    Ok(())
}
