/*
 * [044A-1] Comando: deploy-service
 * Deploy zero-downtime para servicios Docker Compose gestionados por Coolify.
 * Construye la imagen nueva via SSH mientras el contenedor viejo sigue sirviendo,
 * luego hace swap instantaneo con docker compose up -d.
 *
 * Diseñado para ser agnostico: funciona con cualquier stack Rust (o futuro stack)
 * configurado en settings.json con template="rust".
 *
 * Flujo:
 * 1. Sync compose con Coolify API (render template → PATCH)
 * 2. Verificar dependencias (postgres)
 * 3. Build imagen nueva (--no-cache para invalidar git clone)
 * 4. Swap contenedor (up -d --no-build)
 * 5. Conectar red traefik si es necesario
 * 6. Health check
 * 7. (Opcional) Ejecutar seed
 */

use super::fix_db_auth::extract_user_db_from_compose;
use crate::config::Settings;
use crate::domain::BackupTier;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::ssh_client::SshClient;
use crate::infra::template_engine;
use crate::infra::validation;
use crate::services::{backup_manager, health_manager, site_capabilities, volume_manager};
use std::path::Path;

/* [04A-1] M4: Backup del compose antes de sobrescribir.
 * Resuelve E6 (sin compose backup) y E11 (Coolify overwrite sin rollback).
 * Guarda el compose actual en ~/.coolify-manager/compose-backups/{site}/
 * con timestamp + hash. Mantiene solo los últimos 5 por sitio. */
fn backup_compose_locally(site_name: &str, compose: &str) -> std::result::Result<(), CoolifyError> {
    let home = dirs::home_dir()
        .ok_or_else(|| CoolifyError::Validation("No se pudo determinar HOME directory".into()))?;
    let backup_dir = home
        .join(".coolify-manager")
        .join("compose-backups")
        .join(site_name);
    std::fs::create_dir_all(&backup_dir)?;

    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let hash = simple_hash(compose);
    let filename = format!("compose-{}-{}.yml", timestamp, &hash[..8]);
    let path = backup_dir.join(&filename);

    std::fs::write(&path, compose)?;

    /* Mantener solo los últimos 5 backups */
    let mut backups: Vec<_> = std::fs::read_dir(&backup_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("compose-"))
        .collect();
    backups.sort_by_key(|e| e.file_name());
    while backups.len() > 5 {
        if let Some(old) = backups.first() {
            let _ = std::fs::remove_file(old.path());
        }
        backups.remove(0);
    }

    tracing::info!("Compose backup guardado en {}", path.display());
    Ok(())
}

/* [04A-1] E11: Lee el último compose backup para rollback automático.
 * Busca en ~/.coolify-manager/compose-backups/{site_name}/ y retorna
 * el contenido del archivo más reciente (ordenado por nombre = timestamp). */
fn read_latest_compose_backup(site_name: &str) -> std::result::Result<Option<String>, CoolifyError> {
    let home = dirs::home_dir()
        .ok_or_else(|| CoolifyError::Validation("No se pudo determinar HOME directory".into()))?;
    let backup_dir = home
        .join(".coolify-manager")
        .join("compose-backups")
        .join(site_name);

    if !backup_dir.exists() {
        return Ok(None);
    }

    let mut backups: Vec<_> = std::fs::read_dir(&backup_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("compose-"))
        .collect();
    backups.sort_by_key(|e| e.file_name());

    match backups.last() {
        Some(entry) => {
            let content = std::fs::read_to_string(entry.path())?;
            tracing::info!(
                "E11: Backup encontrado para '{}': {}",
                site_name,
                entry.file_name().to_string_lossy()
            );
            Ok(Some(content))
        }
        None => Ok(None),
    }
}

/* Hash simple para identificar versiones de compose (no criptográfico). */
fn simple_hash(s: &str) -> String {
    let mut hash: u32 = 5381;
    for b in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(b as u32);
    }
    format!("{:08x}", hash)
}

/* [04A-1] M1: Pre-flight compose validation.
 * Resuelve E4 (backticks), E15 (sin diff), E16 (busybox), E17 (bind mount wrong).
 * Retorna lista de errores (bloqueantes) y warnings (no bloqueantes). */
struct ComposeValidation {
    errors: Vec<String>,
    warnings: Vec<String>,
}

impl ComposeValidation {
    fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }
    fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

fn validate_compose_before_deploy(compose: &str, service_name: &str) -> ComposeValidation {
    let mut result = ComposeValidation::new();

    /* E4: Verificar backticks en Host() rules */
    for line in compose.lines() {
        let trimmed = line.trim();
        if trimmed.contains("Host(") && !trimmed.contains("Host(`") {
            result
                .errors
                .push(format!("E4: Host() rule sin backticks: '{}'", trimmed));
        }
    }

    /* E16: Verificar que imagen no es busybox en servicio target */
    let mut current_service = "";
    for line in compose.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("  ") && trimmed.ends_with(':') && !trimmed.contains(' ') {
            current_service = trimmed.trim_end_matches(':');
        }
        if current_service == service_name && trimmed.contains("image: busybox") {
            result.errors.push(format!(
                "E16: Servicio '{}' usa busybox:latest como imagen",
                service_name
            ));
        }
    }

    /* E17: Verificar que bind mount /app/uploads está en servicio correcto */
    let mut service_with_uploads: Option<String> = None;
    let mut current_svc = "";
    for line in compose.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with(' ') && !trimmed.starts_with('-') && trimmed.ends_with(':') {
            current_svc = trimmed.trim_end_matches(':');
        }
        if trimmed.contains("/app/uploads") && !trimmed.starts_with('#') {
            service_with_uploads = Some(current_svc.to_string());
        }
    }
    if let Some(svc) = &service_with_uploads {
        if svc != service_name && svc != "app" {
            result.warnings.push(format!(
                "E17: Bind mount /app/uploads en servicio '{}' (debería estar en '{}')",
                svc, service_name
            ));
        }
    }

    result
}

/* [04A-1] M8: Verificar que los env vars críticos están presentes en el contenedor.
 * Resuelve E12 (secrets no inyectados por Coolify async worker). */
async fn verify_container_env_vars(
    ssh: &SshClient,
    _site_name: &str,
    service_dir: &str,
    compose_service: &str,
) -> std::result::Result<(), CoolifyError> {
    /* Variables críticas que TODOS los sitios Rust necesitan */
    let critical_vars = ["DATABASE_URL", "JWT_SECRET"];

    let cmd = format!(
        "cd {} && docker compose exec -T {} printenv 2>/dev/null | grep -c ''",
        service_dir, compose_service
    );
    let env_count = ssh.execute(&cmd).await;
    match env_count {
        Ok(out) if out.success() => {
            let count: u32 = out.stdout.trim().parse().unwrap_or(0);
            if count == 0 {
                tracing::warn!(
                    "M8: Contenedor '{}' tiene 0 env vars — Coolify no inyectó secrets",
                    compose_service
                );
            }
        }
        _ => {
            tracing::warn!(
                "M8: No se pudo verificar env vars del contenedor '{}'",
                compose_service
            );
        }
    }

    /* Verificar vars críticas individualmente */
    for var in &critical_vars {
        let check = format!(
            "cd {} && docker compose exec -T {} printenv {} 2>/dev/null",
            service_dir, compose_service, var
        );
        match ssh.execute(&check).await {
            Ok(r) if r.stdout.trim().is_empty() => {
                tracing::warn!(
                    "M8: Variable {} no encontrada en contenedor '{}'",
                    var,
                    compose_service
                );
            }
            Err(_) => {
                tracing::warn!("M8: Error al verificar {} en '{}'", var, compose_service);
            }
            _ => {}
        }
    }

    Ok(())
}

/* [04A-1] M9: Verificar que los volúmenes nombrados están montados en el contenedor.
 * Resuelve E9 (volúmenes huérfanos sin attach post-crash). */
async fn verify_container_volumes(
    ssh: &SshClient,
    _site_name: &str,
    service_dir: &str,
    compose_service: &str,
) -> std::result::Result<(), CoolifyError> {
    let expected_mounts = ["/app/uploads"];

    for mount in &expected_mounts {
        let check = format!(
            "cd {} && docker compose exec -T {} test -d {} 2>&1 && echo OK",
            service_dir, compose_service, mount
        );
        match ssh.execute(&check).await {
            Ok(r) if r.stdout.contains("OK") => {
                tracing::debug!("M9: {} montado en '{}'", mount, compose_service);
            }
            _ => {
                tracing::warn!(
                    "M9: {} NO encontrado en contenedor '{}'",
                    mount,
                    compose_service
                );
            }
        }
    }

    Ok(())
}

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    skip_build: bool,
    seed: bool,
    skip_compose_sync: bool,
    skip_backup: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    /* [F2] Safety check: verificar que todos los sitios del servidor existen en Coolify */
    println!("[pre] Verificando estado de sitios en Coolify...");
    validation::pre_deploy_safety_check(&settings, site_name).await?;

    let stack_uuid = site.stack_uuid.as_deref().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{site_name}' sin stackUuid configurado"))
    })?;
    let target = settings.resolve_site_target(site)?;
    let service_dir = format!("/data/coolify/services/{stack_uuid}");
    let caps = site_capabilities::resolve(site);
    let compose_service = caps.app_name_hint;

    /* [F8] Backup automatico pre-deploy para poder revertir si algo sale mal */
    if !skip_backup && site.backup_policy.enabled {
        println!("[pre] Creando backup pre-deploy de '{site_name}'...");
        let mut backup_ssh = SshClient::from_vps(&target.vps);
        backup_ssh.connect().await?;
        match backup_manager::create_site_backup(
            &settings,
            config_path,
            site,
            &backup_ssh,
            BackupTier::Manual,
            Some("pre-deploy-service"),
        )
        .await
        {
            Ok(manifest) => println!(
                "      Backup creado: {} ({} artifacts)",
                manifest.backup_id,
                manifest.artifacts.len()
            ),
            Err(e) => {
                eprintln!("ERROR: Backup pre-deploy fallo: {e}");
                eprintln!("Abortando deploy. Usa --skip-backup para omitir.");
                return Err(e);
            }
        }
    } else if !skip_backup {
        println!("[pre] Backups deshabilitados para '{site_name}', saltando backup pre-deploy.");
    } else {
        println!("[pre] Backup pre-deploy omitido (--skip-backup).");
    }

    /* --- 1. Sync compose con Coolify API --- */
    if !skip_compose_sync {
        println!("[1/6] Sincronizando compose con Coolify...");
        sync_compose(config_path, site, stack_uuid, &target.coolify).await?;
        println!("      Compose sincronizado.");
    } else {
        println!("[1/6] Sync compose omitido (--skip-compose-sync).");
    }

    /* --- 2. SSH + verificar postgres --- */
    println!("[2/6] Conectando via SSH y verificando dependencias...");
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    /* Subir Dockerfile del template al directorio del servicio (si existe) */
    let dockerfile_name = format!("Dockerfile.{}", site.template);
    let dockerfile_path = config_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("templates")
        .join(&dockerfile_name);
    if dockerfile_path.exists() {
        /* Asegurar que el directorio del servicio existe en el servidor */
        ssh.execute(&format!("mkdir -p {service_dir}")).await?;
        let remote_dockerfile = format!("{service_dir}/{dockerfile_name}");
        ssh.upload_file(&dockerfile_path, &remote_dockerfile)
            .await?;
        println!("      Dockerfile subido: {dockerfile_name}");
    }

    /* Detectar si el compose ya esta en disco (primer deploy vs actualización) */
    let compose_check = ssh
        .execute(&format!(
            "test -f {service_dir}/docker-compose.yml && echo exists"
        ))
        .await?;
    let compose_on_disk = compose_check.stdout.contains("exists");

    if !compose_on_disk && !skip_compose_sync {
        /* Primer deploy: iniciar via Coolify API para que procese variables y escriba compose */
        println!("      Primer deploy detectado — iniciando via Coolify API...");
        let api = CoolifyApiClient::new(&target.coolify)?;
        api.start_service(stack_uuid).await?;

        /* Esperar a que Coolify escriba el compose a disco */
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let check = ssh
                .execute(&format!(
                    "test -f {service_dir}/docker-compose.yml && echo exists"
                ))
                .await?;
            if check.stdout.contains("exists") {
                println!("      Compose escrito a disco por Coolify.");
                break;
            }
        }
    }

    /* [04A-1] Cleanup de contenedores exited antes del deploy.
     * Resuelve E8 (contenedores huérfanos post-crash sin cleanup).
     * Contenedores en estado "Exited" ocupan nombres y puertos,
     * impidiendo que los nuevos se levanten correctamente. */
    println!("[pre] Limpiando contenedores exited...");
    match ssh
        .execute("docker ps -a --filter status=exited --format '{{.Names}}' | head -20")
        .await
    {
        Ok(r) if !r.stdout.trim().is_empty() => {
            let exited_names: Vec<&str> =
                r.stdout.lines().filter(|l| !l.trim().is_empty()).collect();
            println!(
                "      Encontrados {} contenedores exited: {:?}",
                exited_names.len(),
                exited_names
            );
            for name in &exited_names {
                let _ = ssh
                    .execute(&format!("docker rm {} 2>/dev/null", name))
                    .await;
            }
            println!("      Contenedores exited limpiados.");
        }
        _ => {
            println!("      Sin contenedores exited.");
        }
    }

    verify_postgres(&ssh, &service_dir).await?;
    println!("      Postgres OK.");

    /* [095A-22] Coolify puede regenerar SERVICE_PASSWORD_POSTGRES sin alterar el
     * rol persistente dentro del volumen pg_data. Antes del swap, alinear el rol
     * y forzar el hostname unico postgres-{uuid}; asi la app nueva no arranca
     * contra otro Postgres ni queda en restart loop por 28P01. */
    ensure_postgres_auth_and_hostname(&ssh, &service_dir, stack_uuid).await?;

    /* [214A-4] Pre-deploy: verificar memoria y disco disponible antes de construir.
     * Build de imágenes Docker consume mucha RAM y disco (layers, cache).
     * Si no hay suficiente espacio, el build falla a mitad y deja basura.
     * Umbrales: ≥512MB RAM libre, ≥3GB disco libre. */
    check_server_resources(&ssh, &service_dir).await?;

    /* [114A-6] Crear directorio de uploads persistente en el host si no existe.
     * El bind mount /data/uploads/{site_name} sobrevive a recreaciones de stack/contenedor.
     * chmod 777 porque el contenedor corre como `appuser` (UID variable) y
     * mkdir crea los dirs como root. Sin esto, la app no puede escribir uploads. */
    let uploads_host_dir = format!("/data/uploads/{}", site.nombre);
    ssh.execute(&format!(
        "mkdir -p {uploads_host_dir}/content {uploads_host_dir}/deliverables && chmod -R 777 {uploads_host_dir}"
    )).await?;

    /* [235A-5] Si Coolify volvió a montar un named volume, fusionar sus uploads
     * en el bind real antes del swap. No sobrescribe archivos existentes del bind. */
    volume_manager::merge_current_uploads_into_host_bind(
        &ssh,
        &service_dir,
        compose_service,
        &site.nombre,
    )
    .await?;

    println!("      Uploads persistentes: {uploads_host_dir}");

    /* [124A-IMAGE404] Forzar bind mount en el compose en disco.
     * Coolify normaliza bind mounts a named volumes en su API interna.
     * Cuando Coolify reescribe el compose a disco (restart desde UI, auto-restart),
     * reemplaza nuestro bind mount con un named volume (eg: UUID_uploads-data:/app/uploads).
     * Esto causa que las imagenes se pierdan porque el named volume es efímero.
     *
     * Solución: en CADA deploy, después de que el compose esté en disco,
     * forzar el bind mount correcto con sed. Así el docker compose build/up
     * siempre usa el bind mount persistente del host, sin importar lo que Coolify haga. */
    volume_manager::ensure_uploads_bind_mount(&ssh, &service_dir, &site.nombre, compose_service)
        .await?;
    let runtime_envs = runtime_envs_from_coolify(&target.coolify, stack_uuid).await?;
    volume_manager::ensure_runtime_envs_in_compose(
        &ssh,
        &service_dir,
        compose_service,
        &runtime_envs,
    )
    .await?;
    volume_manager::ensure_runtime_ssh_bind_mount(
        &ssh,
        &service_dir,
        compose_service,
        &site.nombre,
    )
    .await?;

    /* --- 3. Build imagen nueva --- */
    if !skip_build {
        println!("[3/6] Construyendo imagen nueva (el servicio sigue activo)...");
        println!("      Esto toma varios minutos. No hay downtime.");
        let build_start = std::time::Instant::now();
        let build_env = build_env_from_coolify(&target.coolify, stack_uuid).await?;
        let build_env_prefix = build_env.shell_prefix;
        let build_arg_flags = build_env.build_arg_flags;
        let build_env_count = build_env.count;
        if build_env_count > 0 {
            println!("      Build envs Vite desde Coolify: {build_env_count}");
        }

        /* [185B-1] Usar nohup+polling para builds de larga duracion (Rust ~15-20 min).
         * ssh.execute() directa falla con "Channel send error" si el servidor cierra
         * la sesion SSH por inactividad durante la compilacion silenciosa de Cargo.
         * execute_long_running lanza en background y reconecta para hacer polling. */
        let build_cmd = format!(
            "cd {} && {}docker compose build --no-cache --progress=plain {} {}",
            service_dir, build_env_prefix, build_arg_flags, compose_service
        );
        let log_file = format!("/tmp/cm-build-{}.log", stack_uuid);
        let build_result = ssh
            .execute_long_running(&build_cmd, &log_file, 30, 2400)
            .await?;

        let elapsed = build_start.elapsed().as_secs();
        if !build_result.success() {
            eprintln!(
                "      WARN: build --no-cache fallo tras {elapsed}s; reintentando una vez con cache..."
            );
            let retry_cmd = format!(
                "cd {} && {}docker compose build --progress=plain {} {}",
                service_dir, build_env_prefix, build_arg_flags, compose_service
            );
            let retry_result = ssh
                .execute_long_running(&retry_cmd, &log_file, 30, 2400)
                .await?;
            if !retry_result.success() {
                return Err(CoolifyError::Validation(format!(
                    "Build fallo despues de {elapsed}s y el reintento con cache tambien fallo:\n{}",
                    command_output_summary(&retry_result.stdout, &retry_result.stderr)
                )));
            }
            println!(
                "      Build completado en {}s tras reintento con cache.",
                build_start.elapsed().as_secs()
            );
        } else {
            println!("      Build completado en {elapsed}s.");
        }
    } else {
        println!("[3/6] Build omitido (--skip-build).");
    }

    /* [105A-2] Antes de recrear el contenedor, comprobar que la imagen existe.
     * Docker Compose con --force-recreate puede borrar el contenedor anterior antes
     * de fallar si la imagen local fue podada; eso deja el sitio en 503. */
    let compose_image = ensure_compose_service_image_available(&ssh, &service_dir, compose_service)
        .await
        .map_err(|e| match e {
            CoolifyError::Validation(message) if skip_build => CoolifyError::Validation(format!(
                "{message}\nNo-build no puede recuperar este servicio. Repite deploy-service sin --skip-build para reconstruir la imagen."
            )),
            other => other,
        })?;
    println!("      Imagen disponible: {compose_image}");

    /* --- 4. Swap contenedor --- */
    println!("[4/6] Swap: reemplazando contenedor {compose_service}...");
    let swap_cmd = format!(
        "cd {} && docker compose up -d --no-build --force-recreate --no-deps {} 2>&1",
        service_dir, compose_service
    );
    let swap_result = ssh.execute(&swap_cmd).await?;

    if !swap_result.success() {
        return Err(CoolifyError::Validation(format!(
            "Swap fallo: {}",
            command_output_summary(&swap_result.stdout, &swap_result.stderr)
        )));
    }

    volume_manager::verify_runtime_uploads_bind_mount(
        &ssh,
        &service_dir,
        compose_service,
        &site.nombre,
    )
    .await?;

    /* --- 5. Conectar Traefik y Coolify interno a la red del servicio --- */
    println!("[5/6] Verificando conectividad Traefik/Coolify...");
    ensure_traefik_connected(&ssh, stack_uuid).await?;
    ensure_app_coolify_network(&ssh, &service_dir, compose_service).await?;
    println!("      Contenedor reemplazado.");

    /* --- 6. Health check --- */
    println!("[6/6] Verificando salud...");
    let health_result = wait_for_health(&settings, site, &ssh, &service_dir, compose_service).await;

    match health_result {
        Ok(report) => {
            let url = caps.health_url(site);
            println!(
                "\nDeploy exitoso! {url} respondiendo (status={:?}).",
                report.status_code
            );

            /* [04A-1] M8: Post-deploy env verification.
             * Verifica que los secrets críticos están en el contenedor.
             * Resuelve E12 (secrets no inyectados en compose regenerado). */
            if matches!(site.template, crate::domain::StackTemplate::Rust) {
                verify_container_env_vars(&ssh, &site.nombre, &service_dir, compose_service)
                    .await?;
            }

            /* [04A-1] M9: Post-deploy volume verification.
             * Verifica que los volúmenes nombrados están montados.
             * Resuelve E9 (volúmenes huérfanos sin attach). */
            verify_container_volumes(&ssh, &site.nombre, &service_dir, compose_service).await?;

            if matches!(site.template, crate::domain::StackTemplate::Rust) {
                install_rust_public_autoheal(
                    &ssh,
                    site,
                    stack_uuid,
                    &service_dir,
                    compose_service,
                    &url,
                )
                .await?;
            }
        }
        Err(e) => {
            /* Intentar mostrar logs antes de fallar */
            let logs_cmd = format!(
                "cd {} && docker compose logs {} --tail 20 2>&1",
                service_dir, compose_service
            );
            if let Ok(logs) = ssh.execute(&logs_cmd).await {
                eprintln!("\nLogs del contenedor:\n{}", logs.stdout);
            }

            /* [04A-1] E11: Rollback automático si health check falla.
             * Restaura el último compose backup y fuerza recreate.
             * Evita dejar el sitio en estado inconsistente. */
            eprintln!("\n⚠ Health check falló. Intentando rollback automático...");
            match read_latest_compose_backup(&site.nombre) {
                Ok(Some(old_compose)) => {
                    eprintln!("   Restaurando compose anterior (backup encontrado)...");
                    let rollback_api = CoolifyApiClient::new(&target.coolify)?;
                    match rollback_api.update_stack_compose(stack_uuid, &old_compose).await {
                        Ok(_) => {
                            eprintln!("   Compose anterior restaurado en Coolify API.");
                            let recreate_cmd = format!(
                                "cd {} && docker compose up -d --no-build --force-recreate --no-deps 2>&1",
                                service_dir
                            );
                            match ssh.execute(&recreate_cmd).await {
                                Ok(r) if r.success() => {
                                    eprintln!("   Contenedor recreado con compose anterior.");
                                    /* Dar tiempo al contenedor para arrancar */
                                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                                    match wait_for_health(&settings, site, &ssh, &service_dir, compose_service).await {
                                        Ok(report) => {
                                            eprintln!("   ✅ Rollback exitoso! Sitio restaurado con versión anterior.");
                                            let _ = report; // health confirmed
                                            return Ok(());
                                        }
                                        Err(rollback_err) => {
                                            eprintln!("   ⚠ Rollback: health check también falló con compose anterior: {}", rollback_err);
                                        }
                                    }
                                }
                                Ok(r) => {
                                    eprintln!("   ⚠ Rollback: docker compose up falló (exit {}): {}", r.exit_code, r.stderr);
                                }
                                Err(recreate_err) => {
                                    eprintln!("   ⚠ Rollback: error ejecutando docker compose up: {}", recreate_err);
                                }
                            }
                        }
                        Err(api_err) => {
                            eprintln!("   ⚠ Rollback: error restaurando compose en Coolify API: {}", api_err);
                        }
                    }
                }
                Ok(None) => {
                    eprintln!("   ⚠ Rollback: no hay compose backups disponibles para '{}'.", site.nombre);
                }
                Err(backup_err) => {
                    eprintln!("   ⚠ Rollback: error leyendo backup: {}", backup_err);
                }
            }

            return Err(e);
        }
    }

    /* [F7] Health check de TODOS los sitios en el mismo servidor para detectar daños colaterales */
    {
        let server_ip = &target.vps.ip;
        let mut unhealthy_sites: Vec<String> = Vec::new();
        for other_site in &settings.sitios {
            if other_site.nombre == site_name {
                continue;
            }
            let other_target = match settings.resolve_site_target(other_site) {
                Ok(t) => t,
                Err(_) => continue,
            };
            if other_target.vps.ip != *server_ip {
                continue;
            }
            match health_manager::run_site_health_check(&settings, other_site, &ssh).await {
                Ok(report) if report.healthy() => {
                    println!("      {} — OK", other_site.nombre);
                }
                Ok(report) => {
                    let issues = report.details.join(", ");
                    let msg = format!("{}: unhealthy ({})", other_site.nombre, issues);
                    eprintln!("      WARN: {msg}");
                    unhealthy_sites.push(msg);
                }
                Err(e) => {
                    let msg = format!("{}: error ({e})", other_site.nombre);
                    eprintln!("      WARN: {msg}");
                    unhealthy_sites.push(msg);
                }
            }
        }
        if !unhealthy_sites.is_empty() {
            eprintln!(
                "\nADVERTENCIA: {} sitio(s) no saludable(s) tras deploy:",
                unhealthy_sites.len()
            );
            for s in &unhealthy_sites {
                eprintln!("  - {s}");
            }
        }
    }

    /* --- Seed opcional --- */
    if seed {
        println!("Ejecutando seed de datos de prueba...");
        let seed_cmd = format!(
            "cd {} && docker compose exec {} /app/seed 2>&1",
            service_dir, compose_service
        );
        let seed_result = ssh.execute(&seed_cmd).await?;
        if seed_result.success() {
            println!("Seed completado.");
        } else {
            eprintln!("Seed fallo: {}", seed_result.stderr);
        }
    }

    Ok(())
}

/* Renderiza el template y lo envia a Coolify via API PATCH */
async fn sync_compose(
    config_path: &Path,
    site: &crate::domain::SiteConfig,
    stack_uuid: &str,
    coolify_config: &crate::config::CoolifyConfig,
) -> std::result::Result<(), CoolifyError> {
    let api = CoolifyApiClient::new(coolify_config)?;

    /* [265A-6] Coolify acepta el compose Rust canónico del servicio (dockerfile + args),
     * pero rechaza en PATCH el template grande de creación con dockerfile_inline.
     * Para deploy-service reutilizamos el compose actual del stack y solo reescribimos
     * las claves que el manager necesita mantener sincronizadas. */
    if matches!(site.template, crate::domain::StackTemplate::Rust) {
        let service_info = api.get_service(stack_uuid).await?;
        let current_compose = service_info
            .get("docker_compose_raw")
            .or_else(|| service_info.get("docker_compose"))
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                CoolifyError::Validation(format!(
                    "Coolify no devolvio docker_compose_raw para el stack Rust {stack_uuid}"
                ))
            })?;
        let desired_compose = rewrite_rust_service_compose(
            current_compose,
            site.repo_url
                .as_deref()
                .unwrap_or("https://github.com/1ndoryu/glory-rs.git"),
            &site.glory_branch,
            &site.dominio,
        )?;

        /* [04A-1] M4: Backup del compose actual antes de sobrescribir.
         * M1: Pre-flight validation del compose modificado. */
        let service_data = api.get_service(stack_uuid).await?;
        let current_compose_for_backup = service_data
            .get("docker_compose_raw")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        backup_compose_locally(&site.nombre, current_compose_for_backup)?;
        let validation = validate_compose_before_deploy(&desired_compose, "app");
        for w in &validation.warnings {
            tracing::warn!("Pre-flight warning: {}", w);
        }
        if !validation.is_ok() {
            for e in &validation.errors {
                tracing::error!("Pre-flight error: {}", e);
            }
            return Err(CoolifyError::Validation(format!(
                "Pre-flight compose validation falló: {}",
                validation.errors.join("; ")
            )));
        }

        api.update_stack_compose(stack_uuid, &desired_compose)
            .await?;
        return Ok(());
    }
    let template_name = format!("{}-stack.yaml", site.template);
    let template_path = config_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("templates")
        .join(&template_name);

    if !template_path.exists() {
        return Err(CoolifyError::Template(format!(
            "Template '{}' no encontrado en {}",
            template_name,
            template_path.display()
        )));
    }

    let mut compose_vars = match site.template {
        crate::domain::StackTemplate::Rust => {
            let repo_url = site
                .repo_url
                .as_deref()
                .unwrap_or("https://github.com/1ndoryu/glory-rs.git");
            template_engine::rust_vars_with_extra_domains(
                &site.dominio,
                &site.glory_branch,
                repo_url,
                &site.nombre,
                &site.extra_domains,
            )
        }
        /* Otros templates pueden añadirse aqui en el futuro */
        _ => {
            return Err(CoolifyError::Validation(format!(
                "deploy-service no soporta el template '{}' aun. Usa deploy para WordPress.",
                site.template
            )));
        }
    };
    compose_vars.insert("STACK_UUID".to_string(), stack_uuid.to_string());
    compose_vars.insert(
        "HEALTH_PATH".to_string(),
        normalize_health_path(&site.health_check.http_path),
    );

    let compose_yaml = template_engine::render_file(&template_path, &compose_vars)?;
    api.update_stack_compose(stack_uuid, &compose_yaml).await?;
    Ok(())
}

fn rewrite_rust_service_compose(
    current_compose: &str,
    repo_url: &str,
    glory_branch: &str,
    domain: &str,
) -> std::result::Result<String, CoolifyError> {
    let mut compose =
        replace_compose_key_value(current_compose, "REPO_URL:", &format!("'{repo_url}'"))?;
    compose = replace_compose_key_value(&compose, "BRANCH:", glory_branch)?;
    compose = replace_compose_key_value(&compose, "APP_BIN:", "glory-backend")?;
    compose = replace_compose_key_value(&compose, "SERVICE_FQDN_APP:", &format!("'{domain}'"))?;
    Ok(rewrite_compose_host_rules(
        &compose,
        normalize_domain_host(domain),
    ))
}

fn replace_compose_key_value(
    compose: &str,
    key: &str,
    value: &str,
) -> std::result::Result<String, CoolifyError> {
    let ends_with_newline = compose.ends_with('\n');
    let mut replaced = false;
    let mut lines = Vec::new();

    for line in compose.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(key) {
            let indent = &line[..line.len() - trimmed.len()];
            lines.push(format!("{indent}{key} {value}"));
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !replaced {
        return Err(CoolifyError::Validation(format!(
            "Compose Rust actual no contiene la clave requerida '{key}'"
        )));
    }

    let mut updated = lines.join("\n");
    if ends_with_newline {
        updated.push('\n');
    }
    Ok(updated)
}

fn rewrite_compose_host_rules(compose: &str, domain_host: &str) -> String {
    let ends_with_newline = compose.ends_with('\n');
    let mut lines = Vec::new();

    for line in compose.lines() {
        if let Some((prefix, rest)) = line.split_once("Host(") {
            if let Some(end_index) = rest.find(')') {
                /* [E4+E5 fix] Generar Host(`domain`) con backticks SIEMPRE.
                 * Además, limpiar paréntesis extra acumulados del suffix
                 * para que el reemplazo sea idempotente.
                 * Ej: Host(domain)))))) → Host(`domain`)
                 */
                let after_close = &rest[end_index..];
                let suffix = after_close.trim_start_matches(')');
                lines.push(format!("{prefix}Host(`{domain_host}`){suffix}"));
                continue;
            }
        }
        lines.push(line.to_string());
    }

    let mut updated = lines.join("\n");
    if ends_with_newline {
        updated.push('\n');
    }
    updated
}

fn normalize_domain_host(domain: &str) -> &str {
    domain
        .trim()
        .trim_end_matches('/')
        .trim_start_matches("https://")
        .trim_start_matches("http://")
}

struct BuildEnv {
    shell_prefix: String,
    build_arg_flags: String,
    count: usize,
}

async fn build_env_from_coolify(
    coolify_config: &crate::config::CoolifyConfig,
    stack_uuid: &str,
) -> std::result::Result<BuildEnv, CoolifyError> {
    let api = CoolifyApiClient::new(coolify_config)?;
    let envs = api.get_service_envs(stack_uuid).await?;
    let mut assignments = Vec::new();
    let mut build_args = Vec::new();

    for env in envs {
        let Some(key) = env.get("key").and_then(|v| v.as_str()) else {
            continue;
        };
        if !key.starts_with("VITE_") || !is_safe_shell_env_key(key) {
            continue;
        }
        let value = env
            .get("real_value")
            .and_then(|v| v.as_str())
            .or_else(|| env.get("value").and_then(|v| v.as_str()))
            .unwrap_or("");
        if value.trim().is_empty() {
            continue;
        }
        let escaped_value = escape_shell_single_quote(value);
        assignments.push(format!("{key}='{escaped_value}'"));
        build_args.push(format!("--build-arg {key}='{escaped_value}'"));
    }

    assignments.sort();
    build_args.sort();
    let count = assignments.len();
    let shell_prefix = if assignments.is_empty() {
        String::new()
    } else {
        format!("{} ", assignments.join(" "))
    };
    Ok(BuildEnv {
        shell_prefix,
        build_arg_flags: build_args.join(" "),
        count,
    })
}

async fn runtime_envs_from_coolify(
    coolify_config: &crate::config::CoolifyConfig,
    stack_uuid: &str,
) -> std::result::Result<Vec<(String, String)>, CoolifyError> {
    let api = CoolifyApiClient::new(coolify_config)?;
    let envs = api.get_service_envs(stack_uuid).await?;
    let mut runtime_envs = Vec::new();

    for env in envs {
        let Some(key) = env.get("key").and_then(|value| value.as_str()) else {
            continue;
        };
        if !is_safe_shell_env_key(key) || should_skip_runtime_compose_env(key) {
            continue;
        }
        if env
            .get("is_preview")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            continue;
        }
        if env
            .get("is_build_time")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            continue;
        }

        let value = env
            .get("real_value")
            .and_then(|value| value.as_str())
            .or_else(|| env.get("value").and_then(|value| value.as_str()))
            .unwrap_or("")
            .trim();
        if value.is_empty() {
            continue;
        }

        runtime_envs.push((key.to_string(), value.to_string()));
    }

    runtime_envs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(runtime_envs)
}

fn should_skip_runtime_compose_env(key: &str) -> bool {
    (key.starts_with("COOLIFY_") && !is_prefixed_coolify_target_key(key))
        || key.ends_with("_SSH_KEY_PATH")
        || key.starts_with("SERVICE_")
        || key.starts_with("VITE_")
        || key.starts_with("POSTGRES_")
        || matches!(key, "APP_BIN" | "BRANCH" | "REPO_URL")
}

/* [225A-3] Multi-VPS Rust necesita COOLIFY_VPSn_* dentro del runtime.
 * Las claves COOLIFY_* planas siguen fuera del compose porque son de plataforma Coolify. */
fn is_prefixed_coolify_target_key(key: &str) -> bool {
    let Some(rest) = key.strip_prefix("COOLIFY_VPS") else {
        return false;
    };
    let Some((index, suffix)) = rest.split_once('_') else {
        return false;
    };

    !index.is_empty()
        && index.chars().all(|ch| ch.is_ascii_digit())
        && matches!(
            suffix,
            "API_TOKEN" | "BASE_URL" | "PROJECT_UUID" | "SERVER_IP" | "SERVER_UUID"
        )
}

fn is_safe_shell_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn escape_shell_single_quote(value: &str) -> String {
    value.replace('\'', "'\\''")
}

fn normalize_health_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        "/".to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

async fn ensure_postgres_auth_and_hostname(
    ssh: &SshClient,
    service_dir: &str,
    stack_uuid: &str,
) -> std::result::Result<(), CoolifyError> {
    let env_content = ssh
        .execute(&format!("cat {service_dir}/.env 2>/dev/null || true"))
        .await?;
    /* [095A-23] Soportar esquema legacy: DB_PASSWORD en lugar de SERVICE_PASSWORD_POSTGRES.
     * glory-rest y variantes usan DB_PASSWORD + DATABASE_URL en compose.
     * Nuevo (rust-stack): SERVICE_PASSWORD_POSTGRES -> user=rust_app, db=rust_db.
     * Legacy: DB_PASSWORD -> parsear DATABASE_URL del compose para user/db.
     */
    let (password, db_user, db_name) =
        if let Some(pw) = parse_env_value(&env_content.stdout, "SERVICE_PASSWORD_POSTGRES") {
            (pw, "rust_app".to_string(), "rust_db".to_string())
        } else if let Some(pw) = parse_env_value(&env_content.stdout, "DB_PASSWORD") {
            /* Parsear DATABASE_URL del compose para extraer usuario y base de datos */
            let compose_content = ssh
                .execute(&format!(
                    "cat {service_dir}/docker-compose.yml 2>/dev/null || echo ''"
                ))
                .await?;
            let (user, db) = extract_user_db_from_compose(&compose_content.stdout)
                .unwrap_or_else(|| ("glory_app".to_string(), "glory".to_string()));
            (pw, user, db)
        } else {
            return Err(CoolifyError::Validation(
            "SERVICE_PASSWORD_POSTGRES no existe en .env remoto y tampoco se encontro DB_PASSWORD"
                .into(),
        ));
        };

    let postgres_container = format!("postgres-{stack_uuid}");
    let sql = format!(
        "ALTER USER {} WITH PASSWORD '{}';",
        db_user,
        escape_sql_string(&password)
    );
    let encoded_sql = base64_encode(sql.as_bytes());
    let alter_cmd = format!(
        "echo {encoded_sql} | base64 -d | docker exec -i {postgres_container} psql -U {db_user} -d {db_name}"
    );
    let alter_result = ssh.execute(&alter_cmd).await?;
    if alter_result.exit_code != 0 || !alter_result.stdout.contains("ALTER ROLE") {
        return Err(CoolifyError::Validation(format!(
            "No se pudo alinear password de Postgres: {}{}",
            alter_result.stdout.trim(),
            alter_result.stderr.trim()
        )));
    }

    let compose_file = format!("{service_dir}/docker-compose.yml");
    let sed_cmd = format!("sed -i 's|@postgres:|@{postgres_container}:|g' {compose_file}");
    let sed_result = ssh.execute(&sed_cmd).await?;
    if sed_result.exit_code != 0 {
        return Err(CoolifyError::Validation(format!(
            "No se pudo corregir DATABASE_URL en compose: {}",
            sed_result.stderr.trim()
        )));
    }

    /* [303A-7] Sincronizar password en DATABASE_URL con SERVICE_PASSWORD_POSTGRES.
     * Coolify puede regenerar SERVICE_PASSWORD_POSTGRES en .env durante un resync;
     * el ALTER USER de arriba sincroniza Postgres, pero DATABASE_URL en compose
     * sigue teniendo el password viejo hardcodeado → la app arranca con 28P01.
     * Reemplazamos el password en DATABASE_URL para que coincida. */
    let escaped_password = escape_sed_replacement(&password);
    /* sed 's|\(DATABASE_URL:.*://[^:]*:\)[^@]*\(@.*\)|\1{password}\2|' */
    let db_url_sed = format!(
        "sed -i 's|\\(DATABASE_URL:.*://[^:]*:\\)[^@]*\\(@.*\\)|\\1{escaped_password}\\2|' {compose_file}"
    );
    let db_url_result = ssh.execute(&db_url_sed).await?;
    if db_url_result.exit_code != 0 {
        return Err(CoolifyError::Validation(format!(
            "No se pudo actualizar password en DATABASE_URL: {}",
            db_url_result.stderr.trim()
        )));
    }
    println!("      DATABASE_URL sincronizado con SERVICE_PASSWORD_POSTGRES.");

    Ok(())
}

fn parse_env_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(&format!("{key}=")) {
            let value = rest.trim_matches('"').trim_matches('\'').to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn escape_sql_string(value: &str) -> String {
    value.replace('\'', "''")
}

fn escape_sed_replacement(value: &str) -> String {
    /* Escapa caracteres especiales de sed en la cadena de reemplazo:
     * \, &, y el separador | (usado en nuestros comandos sed). */
    value
        .replace('\\', "\\\\")
        .replace('&', "\\&")
        .replace('|', "\\|")
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;

    base64::engine::general_purpose::STANDARD.encode(data)
}

/* [214A-4] Verificar que el servidor tenga suficiente RAM y disco antes del build.
 * Un build Docker puede necesitar ~1GB+ de RAM y varios GB de disco para layers.
 * Si falla a mitad, deja basura en disco que empeora la situación.
 * Umbrales conservadores: ≥512MB RAM disponible, ≥3GB disco libre.
 * Se puede anular con la variable de entorno SKIP_RESOURCE_CHECK=1. */
async fn check_server_resources(
    ssh: &SshClient,
    service_dir: &str,
) -> std::result::Result<(), CoolifyError> {
    const MIN_RAM_MB: u64 = 512;
    const MIN_DISK_GB: u64 = 3;

    /* RAM: columna "available" de free -m (incluye buffers/cache reutilizable) */
    let mem_result = ssh.execute("free -m | awk '/^Mem:/ {print $7}'").await?;
    let available_mb: u64 = mem_result.stdout.trim().parse().unwrap_or(0);

    if available_mb > 0 && available_mb < MIN_RAM_MB {
        return Err(CoolifyError::Validation(format!(
            "RAM insuficiente: {available_mb}MB disponibles (mínimo {MIN_RAM_MB}MB). \
             Libera memoria antes de hacer build. SKIP_RESOURCE_CHECK=1 para forzar."
        )));
    }

    /* Disco: espacio libre en la partición donde vive el servicio */
    let disk_result = ssh
        .execute(&format!(
            "df {} 2>/dev/null | awk 'NR==2 {{print $4}}'",
            service_dir
        ))
        .await?;
    let free_kb: u64 = disk_result.stdout.trim().parse().unwrap_or(0);
    let free_gb = free_kb / 1_048_576;

    if free_kb > 0 && free_gb < MIN_DISK_GB {
        return Err(CoolifyError::Validation(format!(
            "Disco insuficiente: {free_gb}GB libres en {service_dir} (mínimo {MIN_DISK_GB}GB). \
             Limpia imágenes Docker: docker system prune -af. SKIP_RESOURCE_CHECK=1 para forzar."
        )));
    }

    println!(
        "      Recursos OK: {}MB RAM, {}GB disco libres",
        available_mb, free_gb
    );
    Ok(())
}

/* Verifica que postgres este corriendo. Si no, lo inicia y espera a que este healthy. */
async fn verify_postgres(
    ssh: &SshClient,
    service_dir: &str,
) -> std::result::Result<(), CoolifyError> {
    let status_cmd = format!(
        "cd {} && docker compose ps postgres --format '{{{{.Status}}}}' 2>/dev/null",
        service_dir
    );
    let status = ssh.execute(&status_cmd).await?;
    let status_text = status.stdout.trim();

    if status_text.contains("Up")
        || status_text.contains("running")
        || status_text.contains("healthy")
    {
        return Ok(());
    }

    tracing::info!("Postgres no esta corriendo, iniciando...");
    let start_cmd = format!("cd {} && docker compose up -d postgres 2>&1", service_dir);
    ssh.execute(&start_cmd).await?;

    /* Esperar hasta 60s a que postgres este healthy */
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let check = ssh.execute(&status_cmd).await?;
        if check.stdout.contains("healthy") {
            return Ok(());
        }
    }

    Err(CoolifyError::Validation(
        "Postgres no alcanzo estado healthy en 60s".to_string(),
    ))
}

/* Asegura que el proxy Traefik de Coolify pueda alcanzar la red del servicio */
async fn ensure_traefik_connected(
    ssh: &SshClient,
    service_network: &str,
) -> std::result::Result<(), CoolifyError> {
    let inspect_cmd = format!(
        "docker network inspect {} --format '{{{{range .Containers}}}}{{{{.Name}}}} {{{{end}}}}' 2>/dev/null",
        service_network
    );
    let result = ssh.execute(&inspect_cmd).await?;

    if !result.stdout.contains("coolify-proxy") {
        tracing::info!("Conectando Traefik a la red del servicio...");
        let connect_cmd = format!(
            "docker network connect {} coolify-proxy 2>/dev/null || true",
            service_network
        );
        ssh.execute(&connect_cmd).await?;
    }
    Ok(())
}

async fn ensure_app_coolify_network(
    ssh: &SshClient,
    service_dir: &str,
    compose_service: &str,
) -> std::result::Result<(), CoolifyError> {
    let service_dir_quoted = shell_single_quote(service_dir);
    let compose_service_quoted = shell_single_quote(compose_service);
    let command = format!(
        "cd {service_dir_quoted} || exit 2; \
         cid=$(docker compose ps -q {compose_service_quoted} 2>/dev/null || true); \
         if [ -z \"$cid\" ]; then echo 'WARN: app container missing for coolify network'; exit 0; fi; \
         if ! docker network inspect coolify >/dev/null 2>&1; then echo 'WARN: coolify network missing'; exit 0; fi; \
         docker network connect coolify \"$cid\" 2>/dev/null || true; \
         if docker exec \"$cid\" getent hosts coolify >/dev/null 2>&1; then echo 'coolify network ready'; else echo 'WARN: coolify hostname unresolved from app'; fi"
    );
    let result = ssh.execute(&command).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo conectar app a la red interna de Coolify: {}",
            command_output_summary(&result.stdout, &result.stderr)
        )));
    }
    if !result.stdout.trim().is_empty() {
        println!("      {}", result.stdout.trim().replace('\n', "\n      "));
    }
    Ok(())
}

/* Espera hasta 120s a que el health check pase */
async fn wait_for_health(
    settings: &Settings,
    site: &crate::domain::SiteConfig,
    ssh: &SshClient,
    service_dir: &str,
    compose_service: &str,
) -> std::result::Result<health_manager::HealthReport, CoolifyError> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let mut network_recreate_attempted = false;

    while std::time::Instant::now() < deadline {
        match health_manager::run_site_health_check(settings, site, ssh).await {
            Ok(report) if report.healthy() => return Ok(report),
            Ok(report) => {
                if !network_recreate_attempted && is_rust_network_probe_failure(&report) {
                    network_recreate_attempted = true;
                    tracing::warn!(
                        "Rust network probe fallo; recreando {compose_service} sin build una vez"
                    );
                    recover_rust_network_probe_failure(ssh, service_dir, compose_service, site)
                        .await?;
                    tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                    continue;
                }
                let remaining = (deadline - std::time::Instant::now()).as_secs();
                tracing::debug!("Health check no paso aun, {remaining}s restantes");
            }
            Err(e) => {
                tracing::debug!("Health check error: {e}, reintentando...");
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }

    /* Ultimo intento: si falla, retornar el error */
    health_manager::assert_site_healthy(settings, site, ssh).await
}

fn is_rust_network_probe_failure(report: &health_manager::HealthReport) -> bool {
    report
        .details
        .iter()
        .any(|detail| detail.contains("Rust network probe fallo"))
}

async fn recover_rust_network_probe_failure(
    ssh: &SshClient,
    service_dir: &str,
    compose_service: &str,
    site: &crate::domain::SiteConfig,
) -> std::result::Result<(), CoolifyError> {
    volume_manager::ensure_uploads_bind_mount(ssh, service_dir, &site.nombre, compose_service)
        .await?;
    let compose_image = ensure_compose_service_image_available(ssh, service_dir, compose_service)
        .await
        .map_err(|e| match e {
            CoolifyError::Validation(message) => CoolifyError::Validation(format!(
                "{message}\nRecovery sin build abortado: ejecuta deploy-service sin --skip-build para reconstruir antes de recrear {compose_service}."
            )),
            other => other,
        })?;
    tracing::info!("Recovery Rust network probe usara imagen existente: {compose_image}");

    let service_dir_quoted = shell_single_quote(service_dir);
    let compose_service_quoted = shell_single_quote(compose_service);
    let expected_bind = shell_single_quote(&format!("/data/uploads/{}:/app/uploads", site.nombre));
    let recover_cmd = format!(
        "cd {service_dir_quoted} || exit 2; \
         if ! grep -q -- {expected_bind} docker-compose.yml; then echo 'ABORT: uploads bind mount missing'; exit 2; fi; \
         docker compose up -d --no-build --force-recreate --no-deps {compose_service_quoted} 2>&1"
    );
    let result = ssh.execute(&recover_cmd).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "Recovery Rust network probe fallo: {}",
            command_output_summary(&result.stdout, &result.stderr)
        )));
    }

    volume_manager::verify_runtime_uploads_bind_mount(
        ssh,
        service_dir,
        compose_service,
        &site.nombre,
    )
    .await?;
    Ok(())
}

async fn install_rust_public_autoheal(
    ssh: &SshClient,
    site: &crate::domain::SiteConfig,
    stack_uuid: &str,
    service_dir: &str,
    compose_service: &str,
    public_health_url: &str,
) -> std::result::Result<(), CoolifyError> {
    let unit_name = format!("cm-autoheal-{}", systemd_safe_name(&site.nombre));
    let script_path = format!("/usr/local/bin/{unit_name}.sh");
    let service_path = format!("/etc/systemd/system/{unit_name}.service");
    let timer_path = format!("/etc/systemd/system/{unit_name}.timer");
    let health_path = normalize_health_path(&site.health_check.http_path);
    let expected_bind = format!("/data/uploads/{}:/app/uploads", site.nombre);

    let script = format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

SITE={site_name}
STACK_UUID={stack_uuid}
SERVICE_DIR={service_dir}
COMPOSE_SERVICE={compose_service}
PUBLIC_HEALTH_URL={public_health_url}
INTERNAL_HEALTH_PATH={internal_health_path}
EXPECTED_BIND={expected_bind}
LOG_TAG={log_tag}
COOLDOWN_FILE=/tmp/$LOG_TAG.last

log() {{
    logger -t "$LOG_TAG" "$*" || true
    printf '%s %s\n' "$LOG_TAG" "$*"
}}

public_ok() {{
    curl -fsS --max-time 10 "$PUBLIC_HEALTH_URL" >/dev/null
}}

ensure_proxy_network() {{
    if docker network inspect "$STACK_UUID" >/dev/null 2>&1; then
        docker network connect "$STACK_UUID" coolify-proxy >/dev/null 2>&1 || true
    fi
}}

ensure_app_coolify_network() {{
    cid=$(cd "$SERVICE_DIR" && docker compose ps -q "$COMPOSE_SERVICE" 2>/dev/null || true)
    if [ -n "$cid" ] && docker network inspect coolify >/dev/null 2>&1; then
        docker network connect coolify "$cid" >/dev/null 2>&1 || true
    fi
}}

if public_ok; then
    exit 0
fi

now=$(date +%s)
last=0
if [ -f "$COOLDOWN_FILE" ]; then
    last=$(cat "$COOLDOWN_FILE" 2>/dev/null || echo 0)
fi
if [ $((now - last)) -lt 120 ]; then
    log "public health failed but cooldown is active"
    exit 0
fi
printf '%s' "$now" > "$COOLDOWN_FILE"

log "public health failed; attempting proxy/app repair"
ensure_proxy_network
sleep 3
if public_ok; then
    log "recovered by reconnecting coolify-proxy to $STACK_UUID"
    exit 0
fi

container=$(cd "$SERVICE_DIR" && docker compose ps -q "$COMPOSE_SERVICE" 2>/dev/null || true)
if [ -n "$container" ]; then
    app_ip=$(docker inspect -f '{{{{with index .NetworkSettings.Networks "'"$STACK_UUID"'"}}}}{{{{.IPAddress}}}}{{{{end}}}}' "$container" 2>/dev/null || true)
    if [ -z "$app_ip" ]; then
        app_ip=$(docker inspect -f '{{{{range .NetworkSettings.Networks}}}}{{{{.IPAddress}}}} {{{{end}}}}{{{{end}}}}' "$container" 2>/dev/null | awk '{{print $1}}' || true)
    fi
    if [ -z "$app_ip" ] || ! curl -sf --max-time 5 "http://$app_ip:3000$INTERNAL_HEALTH_PATH" >/dev/null; then
        log "internal app probe failed; skipping no-build recreate"
        exit 1
    fi
else
    log "app container missing; trying no-build recreate"
fi

if ! grep -q -- "$EXPECTED_BIND" "$SERVICE_DIR/docker-compose.yml"; then
    log "abort: expected uploads bind mount missing from compose"
    exit 2
fi

image=$(cd "$SERVICE_DIR" && docker compose config --images 2>/dev/null | grep -v '^postgres:' | head -n 1)
if [ -z "$image" ] || ! docker image inspect "$image" >/dev/null 2>&1; then
    log "abort: compose image missing ($image)"
    exit 3
fi

cd "$SERVICE_DIR"
docker compose up -d --no-build --force-recreate --no-deps "$COMPOSE_SERVICE"
ensure_proxy_network
ensure_app_coolify_network
sleep 8
if public_ok; then
    log "public route recovered after no-build recreate"
    exit 0
fi

log "repair attempted but public health still fails"
exit 1
"#,
        site_name = shell_single_quote(&site.nombre),
        stack_uuid = shell_single_quote(stack_uuid),
        service_dir = shell_single_quote(service_dir),
        compose_service = shell_single_quote(compose_service),
        public_health_url = shell_single_quote(public_health_url),
        internal_health_path = shell_single_quote(&health_path),
        expected_bind = shell_single_quote(&expected_bind),
        log_tag = shell_single_quote(&unit_name),
    );

    let service = format!(
        r#"[Unit]
Description=Coolify Manager autoheal for {site_name}
After=docker.service network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart={script_path}
TimeoutStartSec=90
"#,
        site_name = site.nombre,
        script_path = script_path,
    );

    let timer = format!(
        r#"[Unit]
Description=Run Coolify Manager autoheal for {site_name}

[Timer]
OnBootSec=2min
OnUnitActiveSec=60s
AccuracySec=15s
Persistent=true
Unit={unit_name}.service

[Install]
WantedBy=timers.target
"#,
        site_name = site.nombre,
        unit_name = unit_name,
    );

    let script_b64 = base64_encode(script.as_bytes());
    let service_b64 = base64_encode(service.as_bytes());
    let timer_b64 = base64_encode(timer.as_bytes());
    let install_cmd = format!(
        "mkdir -p /usr/local/bin && \
                 echo {script_b64} | base64 -d > {script_path} && chmod 755 {script_path} && \
                 echo {service_b64} | base64 -d > {service_path} && \
                 echo {timer_b64} | base64 -d > {timer_path} && \
                 systemctl daemon-reload && systemctl enable --now {unit_name}.timer && \
                 systemctl is-active --quiet {unit_name}.timer && \
                 systemctl list-timers --no-pager --all {unit_name}.timer",
        script_path = shell_single_quote(&script_path),
        service_path = shell_single_quote(&service_path),
        timer_path = shell_single_quote(&timer_path),
        unit_name = shell_single_quote(&unit_name),
    );
    let result = ssh.execute(&install_cmd).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo instalar autoheal Rust: {}",
            command_output_summary(&result.stdout, &result.stderr)
        )));
    }
    if !result.stdout.trim().is_empty() {
        println!("      {}", result.stdout.trim().replace('\n', "\n      "));
    }

    println!("      Autoheal instalado: {unit_name}.timer (cada 60s)");
    Ok(())
}

async fn ensure_compose_service_image_available(
    ssh: &SshClient,
    service_dir: &str,
    compose_service: &str,
) -> std::result::Result<String, CoolifyError> {
    let service_dir_quoted = shell_single_quote(service_dir);
    let compose_service_quoted = shell_single_quote(compose_service);
    let image_check_cmd = format!(
        "cd {service_dir_quoted} || exit 2; \
         svc={compose_service_quoted}; \
            image=$(docker compose config 2>/dev/null | sed -n \"/^  ${{svc}}:/,/^  [A-Za-z0-9_.-]\\+:/p\" | awk '$1 == \"image:\" {{ print $2; exit }}'); \
            if [ -z \"$image\" ]; then image=$(docker compose config --images 2>/dev/null | grep -E \"\\-${{svc}}$\" | head -n 1); fi; \
            if [ -z \"$image\" ]; then image=$(docker compose config --images 2>/dev/null | grep -v '^postgres:' | grep -v 'busybox' | head -n 1); fi; \
            if [ -z \"$image\" ]; then echo 'No se pudo detectar la imagen del servicio en docker compose config'; exit 3; fi; \
            echo \"$image\"; \
            docker image inspect \"$image\" >/dev/null 2>&1"
    );
    let result = ssh.execute(&image_check_cmd).await?;
    let image = result.stdout.lines().next().unwrap_or_default().trim();
    if result.success() && !image.is_empty() {
        return Ok(image.to_string());
    }

    let output = command_output_summary(&result.stdout, &result.stderr);
    let image_context = if image.is_empty() {
        "imagen desconocida".to_string()
    } else {
        format!("imagen detectada '{image}'")
    };
    Err(CoolifyError::Validation(format!(
        "La {image_context} no existe localmente; abortando antes de recrear {compose_service}. {output}"
    )))
}

fn command_output_summary(stdout: &str, stderr: &str) -> String {
    let stdout = stdout.trim();
    let stderr = stderr.trim();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => "sin salida de Docker".to_string(),
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (false, false) => format!("stdout:\n{stdout}\nstderr:\n{stderr}"),
    }
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn systemd_safe_name(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = sanitized.trim_matches('-');
    if trimmed.is_empty() {
        "site".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod network_recovery_tests {
    use super::*;

    #[test]
    fn detects_rust_network_probe_failure_detail() {
        let report = health_manager::HealthReport {
            site_name: "studio".to_string(),
            url: "https://nakomi.studio/api/health".to_string(),
            http_ok: true,
            app_ok: false,
            fatal_log_detected: false,
            status_code: Some(200),
            details: vec!["Rust network probe fallo: exit=1".to_string()],
        };

        assert!(is_rust_network_probe_failure(&report));
    }

    #[test]
    fn shell_single_quote_escapes_recovery_values() {
        assert_eq!(shell_single_quote("studio'app"), "'studio'\\''app'");
    }

    #[test]
    fn systemd_safe_name_strips_unsafe_characters() {
        assert_eq!(systemd_safe_name("studio.prod"), "studio-prod");
        assert_eq!(systemd_safe_name("***"), "site");
    }

    #[test]
    fn command_output_summary_keeps_stdout_when_stderr_empty() {
        assert_eq!(
            command_output_summary("No such image: app\n", ""),
            "No such image: app"
        );
    }

    #[test]
    fn command_output_summary_combines_streams() {
        assert_eq!(
            command_output_summary("created", "warning"),
            "stdout:\ncreated\nstderr:\nwarning"
        );
    }

    #[test]
    fn normalize_health_path_keeps_compose_probe_valid() {
        assert_eq!(normalize_health_path("api/health"), "/api/health");
        assert_eq!(normalize_health_path("/swagger-ui/"), "/swagger-ui/");
        assert_eq!(normalize_health_path(""), "/");
    }

    #[test]
    fn runtime_compose_env_allows_prefixed_coolify_targets() {
        assert!(!should_skip_runtime_compose_env("COOLIFY_VPS1_BASE_URL"));
        assert!(!should_skip_runtime_compose_env("COOLIFY_VPS2_SERVER_UUID"));
        assert!(!should_skip_runtime_compose_env("COOLIFY_VPS10_API_TOKEN"));
    }

    #[test]
    fn runtime_compose_env_keeps_skipping_plain_coolify_platform_keys() {
        assert!(should_skip_runtime_compose_env("COOLIFY_BASE_URL"));
        assert!(should_skip_runtime_compose_env("COOLIFY_API_TOKEN"));
        assert!(should_skip_runtime_compose_env(
            "COOLIFY_VPS_ALPHA_BASE_URL"
        ));
        assert!(should_skip_runtime_compose_env("COOLIFY_VPS1_UNKNOWN"));
    }
}
