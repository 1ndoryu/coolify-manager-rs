/* Rollback automático del deploy (119A-5 split deploy_service). */

use super::contexto::CtxDeploy;
use super::*;
use crate::infra::ssh_client::SshClient;

/* [04A-1] E11: Rollback automático si health check falla.
 * Restaura el último compose backup y fuerza recreate.
 * Evita dejar el sitio en estado inconsistente.
 * [119A-2] Descompuesto en R0 (restaurar) + R3/R4/R5 (intentos). */
pub(super) async fn intentar_rollback(ctx: &CtxDeploy<'_>, ssh: &SshClient) {
    eprintln!("\n⚠ Health check falló. Intentando rollback automático...");
    let api = match rollback_restaurar_compose(ctx, ssh).await {
        Some(api) => api,
        None => return,
    };
    if rollback_intento_recreate(ctx, ssh).await {
        return;
    }
    if rollback_intento_rebuild(ctx, ssh).await {
        return;
    }
    if rollback_intento_api(ctx, ssh, &api).await {
        return;
    }
    eprintln!("   ❌ Rollback automático falló en todos los intentos.");
    eprintln!("   El sitio puede estar caído. Verificar manualmente.");
}

/* [119A-2] R0b+R1+R2: restaurar el compose anterior en Coolify API, esperar
 * la regeneración on-disk y corregir el hostname postgres. Devuelve el
 * cliente API para los intentos R3-R5, o None si no se pudo restaurar. */
async fn rollback_restaurar_compose(
    ctx: &CtxDeploy<'_>,
    ssh: &SshClient,
) -> Option<CoolifyApiClient> {
    let site = ctx.site;
    let service_dir = &ctx.service_dir;
    let stack_uuid = ctx.stack_uuid;
    let target = &ctx.target;

    let old_compose = match read_latest_compose_backup(&site.nombre) {
        Ok(Some(compose)) => compose,
        Ok(None) => {
            eprintln!(
                "   ⚠ Rollback: no hay compose backups disponibles para '{}'.",
                site.nombre
            );
            return None;
        }
        Err(backup_err) => {
            eprintln!("   ⚠ Rollback: error leyendo backup: {}", backup_err);
            return None;
        }
    };
    eprintln!("   Restaurando compose anterior (backup encontrado)...");
    /* [incident-2026-07-21] R0b: Inyectar label Traefik en compose restaurado.
     * El backup se guarda ANTES de rewrite_rust_service_compose(), así que
     * puede no tener traefik.docker.network=coolify (sitios legacy).
     * Sin el label, Traefik devuelve 503 "no available server" incluso tras rollback. */
    let old_compose = if matches!(site.template, crate::domain::StackTemplate::Rust) {
        inject_traefik_network_label(&old_compose)
    } else {
        old_compose
    };
    let rollback_api = match CoolifyApiClient::new(&target.coolify) {
        Ok(api) => api,
        Err(api_err) => {
            eprintln!(
                "   ⚠ Rollback: error creando cliente Coolify API: {}",
                api_err
            );
            return None;
        }
    };
    match rollback_api
        .update_stack_compose(stack_uuid, &old_compose)
        .await
    {
        Ok(_) => {
            eprintln!("   Compose anterior restaurado en Coolify API.");

            /* [incident-2026-07-21] R1: Esperar a que Coolify regenere el compose on-disk.
             * update_stack_compose() actualiza la API, pero Coolify necesita tiempo
             * para propagar al archivo docker-compose.yml en disco. Sin esta espera,
             * docker compose up usa el compose PRE-SWAP (nuevo) en vez del restaurado. */
            eprintln!("   Esperando regeneración de compose on-disk (10s)...");
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;

            /* [incident-2026-07-21] R2: Re-ejecutar fix de hostname postgres.
             * El compose backup puede tener @postgres: genérico (legacy).
             * ensure_postgres_auth_and_hostname() corrige a @postgres-{uuid}: y
             * alinea el password. Sin esto, la app no puede conectar a la BD. */
            if matches!(site.template, crate::domain::StackTemplate::Rust) {
                eprintln!("   Corrigiendo hostname postgres en compose restaurado...");
                if let Err(hostname_err) =
                    ensure_postgres_auth_and_hostname(ssh, service_dir, stack_uuid).await
                {
                    eprintln!("   ⚠ Rollback: fix hostname falló: {}", hostname_err);
                }
            }
            Some(rollback_api)
        }
        Err(api_err) => {
            eprintln!(
                "   ⚠ Rollback: error restaurando compose en Coolify API: {}",
                api_err
            );
            None
        }
    }
}

/* [119A-2] R3: Intento 1 — recreate sin build. Devuelve true si el sitio
 * vuelve a healthy con el compose anterior. */
async fn rollback_intento_recreate(ctx: &CtxDeploy<'_>, ssh: &SshClient) -> bool {
    let settings = ctx.settings;
    let site = ctx.site;
    let service_dir = &ctx.service_dir;
    let compose_service = ctx.compose_service.as_str();

    /* [incident-2026-07-21] R3: Intento 1 — recreate sin build */
    let recreate_cmd = format!(
        "cd {} && docker compose up -d --no-build --force-recreate --no-deps {} 2>&1",
        service_dir, compose_service
    );
    let attempt1 = ssh.execute(&recreate_cmd).await;

    match &attempt1 {
        Ok(r) if r.success() => {
            eprintln!("   Contenedor recreado con compose anterior (--no-build).");
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            match wait_for_health(settings, site, ssh, service_dir, compose_service).await {
                Ok(report) => {
                    eprintln!("   ✅ Rollback exitoso! Sitio restaurado con versión anterior.");
                    let _ = report;
                    true
                }
                Err(rollback_err) => {
                    eprintln!("   ⚠ Rollback health (--no-build): {}", rollback_err);
                    false
                }
            }
        }
        Ok(r) => {
            eprintln!(
                "   ⚠ Recreate --no-build fallo (exit {}): {}",
                r.exit_code,
                r.stderr.trim()
            );
            false
        }
        Err(recreate_err) => {
            eprintln!("   ⚠ Recreate --no-build error: {}", recreate_err);
            false
        }
    }
}

/* [119A-2] R4: Intento 2 — recreate CON build (imagen podada).
 * Devuelve true si el sitio vuelve a healthy. */
async fn rollback_intento_rebuild(ctx: &CtxDeploy<'_>, ssh: &SshClient) -> bool {
    let settings = ctx.settings;
    let site = ctx.site;
    let service_dir = &ctx.service_dir;
    let compose_service = ctx.compose_service.as_str();

    /* [incident-2026-07-21] R4: Intento 2 — recreate CON build (imagen podada) */
    eprintln!("   Intentando rollback con rebuild...");
    let rebuild_cmd = format!(
        "cd {} && docker compose up -d --force-recreate --no-deps {} 2>&1",
        service_dir, compose_service
    );
    match ssh.execute(&rebuild_cmd).await {
        Ok(r) if r.success() => {
            eprintln!("   Contenedor recreado con rebuild.");
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            match wait_for_health(settings, site, ssh, service_dir, compose_service).await {
                Ok(report) => {
                    eprintln!("   ✅ Rollback exitoso (con rebuild)! Sitio restaurado.");
                    let _ = report;
                    true
                }
                Err(rb_err) => {
                    eprintln!("   ⚠ Rollback health (rebuild): {}", rb_err);
                    false
                }
            }
        }
        Ok(r) => {
            eprintln!(
                "   ⚠ Rebuild fallo (exit {}): {}",
                r.exit_code,
                r.stderr.trim()
            );
            false
        }
        Err(e2) => {
            eprintln!("   ⚠ Rebuild error: {}", e2);
            false
        }
    }
}

/* [119A-2] R5: Intento 3 — deploy via Coolify API (último recurso).
 * Devuelve true si el sitio vuelve a healthy. */
async fn rollback_intento_api(
    ctx: &CtxDeploy<'_>,
    ssh: &SshClient,
    rollback_api: &CoolifyApiClient,
) -> bool {
    let settings = ctx.settings;
    let site = ctx.site;
    let service_dir = &ctx.service_dir;
    let compose_service = ctx.compose_service.as_str();
    let stack_uuid = ctx.stack_uuid;

    /* [incident-2026-07-21] R5: Intento 3 — deploy via Coolify API (último recurso) */
    eprintln!("   Intentando deploy via Coolify API (último recurso)...");
    match rollback_api.deploy_stack(stack_uuid).await {
        Ok(_) => {
            eprintln!("   Redeploy disparado via API. Esperando (60s)...");
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            match wait_for_health(settings, site, ssh, service_dir, compose_service).await {
                Ok(report) => {
                    eprintln!("   ✅ Rollback exitoso (redeploy API)! Sitio restaurado.");
                    let _ = report;
                    true
                }
                Err(rb_err) => {
                    eprintln!("   ⚠ Rollback health (redeploy API): {}", rb_err);
                    false
                }
            }
        }
        Err(api_err) => {
            eprintln!("   ⚠ Redeploy API fallo: {}", api_err);
            false
        }
    }
}
