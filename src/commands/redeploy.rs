/*
 * Comando: redeploy
 * Fuerza un redeploy del servicio con la estrategia adecuada para cada stack.
 *
 * [065A-5] Para stacks Rust, `redeploy` ya no exige que el usuario conozca la
 * diferencia entre stop+start de Coolify y deploy zero-downtime. Ambos
 * comandos convergen en `deploy-service`, que primero sincroniza el compose
 * correcto y luego construye/swappea sin exponer `/app/uploads` a named
 * volumes vacíos.
 *
 * [124A-IMAGE404] Para stacks no-Rust, el flujo legacy sigue corrigiendo el
 * compose en disco tras el restart de Coolify para impedir que `/app/uploads`
 * termine montado sobre un named volume vacío.
 *
 * [25A-DB-AUTH] Coolify regenera SERVICE_PASSWORD_POSTGRES en cada rebuild
 * del servicio si usa variables de tipo SERVICE_PASSWORD_*. Esto causa que
 * el hash almacenado en el volumen de postgres quede desincronizado con la
 * nueva contraseña del app. fix_db_auth::execute() detecta y corrige el
 * mismatch automáticamente después de cada redeploy.
 */

use crate::commands::{deploy_service, fix_db_auth};
use crate::config::Settings;
use crate::domain::{BackupTier, StackTemplate};
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::{backup_manager, health_manager, site_capabilities, volume_manager};

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    skip_backup: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;
    validation::assert_backup_guardrails(site)?;
    validation::pre_deploy_safety_check(&settings, site_name).await?;

    /* [065A-5] Para stacks Rust, `redeploy` debe ser un alias seguro del flujo
     * zero-downtime. Si exige recordar una semántica distinta a `deploy`, la
     * herramienta falla como UX y además reintroduce el riesgo de perder el
     * bind mount persistente de uploads. */
    if matches!(site.template, crate::domain::StackTemplate::Rust) {
        println!(
            "Sitio '{site_name}' es template Rust — redeploy delega al deploy seguro (sync compose + build + swap)."
        );
        return deploy_service::execute(config_path, site_name, false, false, false, skip_backup)
            .await;
    }

    let stack_uuid = site.stack_uuid.as_deref().unwrap();
    let target = settings.resolve_site_target(site)?;
    let caps = site_capabilities::resolve(site);

    let api = CoolifyApiClient::new(&target.coolify)?;

    tracing::info!("Forzando redeploy de '{site_name}' (uuid: {stack_uuid})");

    /* [045A-GUARDRAILS] Redeploy sin snapshot previo no vuelve a tocar producción.
     * Si el backup falla, se aborta antes del stop/start. */
    if !skip_backup && site.backup_policy.enabled {
        println!("[pre] Creando backup pre-redeploy de '{site_name}'...");
        let mut backup_ssh = SshClient::from_vps(&target.vps);
        backup_ssh.connect().await?;
        let manifest = backup_manager::create_site_backup(
            &settings,
            config_path,
            site,
            &backup_ssh,
            BackupTier::Manual,
            Some("pre-redeploy"),
        )
        .await?;
        println!(
            "      Backup creado: {} ({} artifacts)",
            manifest.backup_id,
            manifest.artifacts.len()
        );
    } else if !skip_backup {
        println!("[pre] Backups deshabilitados para '{site_name}', saltando backup pre-redeploy.");
    } else {
        println!("[pre] Backup pre-redeploy omitido (--skip-backup).");
    }

    /* Stop + Start = redeploy completo (rebuild containers).
     *
     * [504A-STOP-IDEMPOTENT] Si el servicio ya está parado, Coolify devuelve
     * HTTP 400 "already stopped". Se ignora el resultado del stop completamente:
     * lo importante es que el start funcione. Fallar en el stop y abortar deja
     * los contenedores caídos sin recovery automático.
     *
     * [504A-ALREADY-RUNNING] Si el start devuelve HTTP 400 "already running"
     * (auto-restart de Docker), también es inofensivo — el servicio ya corre. */
    let stop_result = api.stop_service(stack_uuid).await;
    match &stop_result {
        Ok(_) => {
            tracing::info!("Servicio detenido. Esperando antes de reiniciar...");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
        Err(e) => {
            tracing::warn!("stop_service retornó error ('{e}'). Puede que ya estuviera parado. Continuando con start...");
        }
    }

    match api.start_service(stack_uuid).await {
        Ok(_) => {}
        Err(ref e) => {
            let msg = e.to_string();
            if msg.contains("already running") || msg.contains("400") {
                tracing::warn!(
                    "start_service retornó '{msg}'. Servicio ya corriendo. Continuando..."
                );
            } else {
                return Err(api.start_service(stack_uuid).await.unwrap_err());
            }
        }
    }

    println!("Redeploy iniciado para '{site_name}'. Esperando estabilizacion...");

    /* Esperar a que Coolify escriba compose y arranque contenedores */
    tokio::time::sleep(std::time::Duration::from_secs(15)).await;

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    /* [124A-IMAGE404] Coolify acaba de reescribir el compose con named volumes.
     * Este fix aplica al template Rust, cuyo contrato persistente es /app/uploads.
     * Kamples/WordPress usan /var/www/html/wp-content/uploads y no deben recibir
     * mounts de /app/uploads insertados por este fallback. */
    let service_dir = format!("/data/coolify/services/{}", stack_uuid);
    if matches!(site.template, StackTemplate::Rust) {
        volume_manager::ensure_uploads_host_dir(&ssh, &site.nombre).await?;
        volume_manager::ensure_uploads_bind_mount(&ssh, &service_dir, &site.nombre).await?;
    }

    /* [504A-NO-BUILD] Stacks con build inline necesitan build local.
     * Usar --no-build solo cuando la imagen existe como imagen publica o persistente. */
    let needs_compose_build =
        caps.requires_local_build || matches!(site.template, StackTemplate::Kamples);
    let build_flag = if needs_compose_build {
        ""
    } else {
        "--no-build"
    };
    let restart_cmd = format!(
        "cd {} && docker compose up -d {} {} 2>&1",
        service_dir, build_flag, caps.app_name_hint
    );
    ssh.execute(&restart_cmd).await?;
    println!("Contenedor reiniciado con bind mount corregido.");

    if matches!(site.template, StackTemplate::Rust) {
        volume_manager::verify_runtime_uploads_bind_mount(
            &ssh,
            &service_dir,
            caps.app_name_hint,
            &site.nombre,
        )
        .await?;
    }

    /* Para Rust, el build puede tardar ~10 min y el primer health puede ser 503.
     * No fallar el comando por un 503 inicial — solo reportar y salir con OK. */
    if caps.requires_local_build {
        println!(
            "Stack con build local: build iniciado en background. Verifica con: health --name {site_name}"
        );
        println!("El build tarda ~10 min. El sitio estará disponible cuando termine.");
        return Ok(());
    }

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

    /* [25A-DB-AUTH] Después de un redeploy, Coolify puede haber regenerado
     * SERVICE_PASSWORD_POSTGRES. Corregir el hash en postgres y el DATABASE_URL
     * automáticamente para evitar que el sitio quede caído por auth failure. */
    println!("Verificando sincronización de credenciales DB post-redeploy...");
    match fix_db_auth::execute(config_path, site_name, false).await {
        Ok(_) => println!("Credenciales DB: OK"),
        Err(e) => {
            /* No bloquear el redeploy si fix_db_auth falla — reportar y continuar */
            eprintln!("WARN: fix-db-auth reportó: {e}");
            eprintln!("      Si el sitio está caído, ejecuta: fix-db-auth --name {site_name}");
        }
    }

    Ok(())
}
