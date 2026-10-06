/*
 * official-deploy — Dispara el deploy oficial de Coolify (`POST /api/v1/deploy`)
 * para un stack existente y espera a que sus contenedores queden Up.
 *
 * [06AA-5] Vía durable del swap laptop: el deploy oficial materializa el
 * `docker-compose-raw` de la API en el host, neutralizando el atributo
 * `image` que `deploy-service`/`redeploy` sobrescriben al regenerar el
 * on-disk con `build:`. Flujo laptop completo: `build-laptop` (carga
 * `cm-local/<sitio>:<tag>`) → `set-compose` (fija `image:` en el raw) →
 * `official-deploy` (materializa y verifica).
 * Garantías por diseño:
 *   1. Resolución SOLO por nombre en settings.json: jamás se acepta un uuid
 *      crudo, así que un typo no puede apuntar a otro stack.
 *   2. `--dry-run` muestra lo que se haría sin tocar host ni API.
 *   3. El poll exige contenedores del stack en estado " Up " (8×30 s, mismo
 *      margen que `cola_kamples_imagen` tras el E2E 2026-10-01); la salud
 *      real la valida el health gate posterior.
 *   4. No compila nada en la VPS: el deploy oficial usa el raw tal cual.
 *
 * [CABLEADO 06-10] Expuesto como `official-deploy --name SITIO [--dry-run]`:
 * variante `Command::OfficialDeploy` (`cli/mod.rs`) + grupo deploy
 * (`cli/dispatch.rs`, `cli/dispatch/deploy.rs`) + `pub mod official_deploy`
 * (`commands/mod.rs`).
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::ssh_client::SshClient;
use crate::services::health_manager;

use std::path::Path;

/* Margen del poll: el pipeline oficial recrea DBs y la app puede pasar por
 * "Restarting" minutos (mismo 8×30 s de `cola_kamples_imagen`). */
const INTENTOS_DEPLOY: usize = 8;
const ESPERA_SEGS: u64 = 30;

/// Parámetros de `official-deploy` (patrón Params de `new_site`/`build_laptop`).
pub struct ParamsOfficialDeploy<'a> {
    pub config_path: &'a Path,
    pub site_name: &'a str,
    pub dry_run: bool,
}

pub async fn execute(params: &ParamsOfficialDeploy<'_>) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(params.config_path)?;
    let site = settings.get_site(params.site_name)?;
    let stack_uuid = site.stack_uuid.clone().ok_or_else(|| {
        CoolifyError::Validation(format!(
            "Sitio '{}' sin stackUuid configurado",
            params.site_name
        ))
    })?;
    let dominio = site.dominio.clone();
    let target = settings.resolve_site_target(site)?;

    println!("Deploy oficial del stack '{}':", params.site_name);
    println!("  uuid:    {stack_uuid}");
    println!("  dominio: {dominio}");

    if params.dry_run {
        println!("[dry-run] Se haría, en orden:");
        println!(
            "  1. POST /api/v1/deploy {{\"uuid\": \"{stack_uuid}\"}} (materializa el raw en el host)"
        );
        println!("  2. Poll {INTENTOS_DEPLOY}x{ESPERA_SEGS}s de contenedores Up del stack");
        println!("  3. Health gate del sitio");
        return Ok(());
    }

    let api = CoolifyApiClient::new(&target.coolify)?;
    api.deploy_stack(&stack_uuid).await?;

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;
    let mut desplegado = false;
    for intento in 1..=INTENTOS_DEPLOY {
        tokio::time::sleep(std::time::Duration::from_secs(ESPERA_SEGS)).await;
        let ps = ssh
            .execute(&format!(
                "docker ps --format '{{{{.Names}}}} {{{{.Image}}}} {{{{.Status}}}}' | grep -i '{stack_uuid}'"
            ))
            .await?;
        if contenedores_up(&ps.stdout) {
            println!("      contenedores Up: OK (intento {intento}/{INTENTOS_DEPLOY}):");
            for linea in ps.stdout.lines() {
                println!("      {linea}");
            }
            desplegado = true;
            break;
        }
        println!("      esperando materialización... (intento {intento}/{INTENTOS_DEPLOY})");
    }
    if !desplegado {
        return Err(CoolifyError::Validation(format!(
            "Coolify no levantó '{}' (Up) tras el deploy oficial",
            params.site_name
        )));
    }
    let report = health_manager::assert_site_healthy(&settings, site, &ssh).await?;
    if report.healthy() {
        println!("Health check: OK — deploy oficial exitoso.");
        Ok(())
    } else {
        for detail in &report.details {
            println!("  - {detail}");
        }
        Err(CoolifyError::Validation(format!(
            "Deploy oficial materializado pero '{}' no pasa el health gate",
            params.site_name
        )))
    }
}

/* El poll exige salida no vacía del grep del stack con algún " Up ". */
fn contenedores_up(salida_ps: &str) -> bool {
    !salida_ps.trim().is_empty() && salida_ps.contains(" Up ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn up_del_stack_pasa() {
        let ps =
            "app-mo4so4440c488g8woow4cow0 mo4so4440c488g8woow4cow0-app Up 26 seconds (healthy)\n";
        assert!(contenedores_up(ps));
    }

    #[test]
    fn vacio_o_sin_up_falla() {
        assert!(!contenedores_up(""));
        assert!(!contenedores_up("   \n"));
        assert!(!contenedores_up(
            "app-x img Created 5 seconds ago\napp-y img Exited (0) 1 minute ago\n"
        ));
    }
}
