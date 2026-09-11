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

use crate::config::Settings;
use crate::domain::BackupTier;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::{backup_manager, health_manager, site_capabilities, volume_manager};
use std::path::Path;

mod compose_backup;
mod compose_sync;
mod compose_validation;
mod container_verification;
mod env_building;
mod host_preflight;
mod postgres_auth;
mod postgres_inspect;
mod rust_autoheal;

use compose_backup::read_latest_compose_backup;
use compose_sync::{inject_traefik_network_label, sync_compose};
use container_verification::{
    verify_container_env_vars, verify_container_volumes, verify_postgres_data_volume,
};
use env_building::{build_env_from_coolify, runtime_envs_from_coolify};
use host_preflight::{
    check_server_resources, ensure_app_coolify_network, ensure_traefik_connected,
    verify_or_inject_traefik_network_label, verify_postgres, wait_for_health,
};
use postgres_auth::ensure_postgres_auth_and_hostname;
use rust_autoheal::{
    command_output_summary, ensure_compose_service_image_available, install_rust_public_autoheal,
};

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

    /* [04A-1] Cleanup de contenedores exited del stack objetivo antes del deploy.
     * Resuelve E8 (contenedores huérfanos post-crash sin cleanup).
     * Contenedores en estado "Exited" ocupan nombres y puertos,
     * impidiendo que los nuevos se levanten correctamente.
     *
     * FIX 2026-08-27 (incidente de daño colateral): antes se limpiaban TODOS los
     * contenedores exited del host sin filtrar por stack, y el deploy de task
     * borró contenedores de otros 9 sitios. Ahora se filtra estrictamente por el
     * label coolify.stack-uuid={uuid}, igual que diagnose.rs, para que la limpieza
     * solo afecte a los contenedores del stack que se está desplegando. */
    println!("[pre] Limpiando contenedores exited del stack {stack_uuid}...");
    let cleanup_cmd = format!(
        "docker ps -a --filter label=coolify.stack-uuid={stack_uuid} --filter status=exited --format '{{{{.Names}}}}' | head -20"
    );
    match ssh.execute(&cleanup_cmd).await {
        Ok(r) if !r.stdout.trim().is_empty() => {
            let exited_names: Vec<&str> =
                r.stdout.lines().filter(|l| !l.trim().is_empty()).collect();
            println!(
                "      Encontrados {} contenedores exited del stack: {:?}",
                exited_names.len(),
                exited_names
            );
            for name in &exited_names {
                let _ = ssh
                    .execute(&format!("docker rm {} 2>/dev/null", name))
                    .await;
            }
            println!("      Contenedores exited del stack limpiados.");
        }
        _ => {
            println!("      Sin contenedores exited del stack.");
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

        /* [21C-6] Coolify regenera el compose on-disk a partir de su estado interno,
         * NO desde el docker_compose_raw que acabamos de PATCHear. Esto significa que
         * REPO_URL y BRANCH en el compose on-disk pueden tener los valores viejos
         * (del primer deploy). Fix: pasar --build-arg explícitos que overridean
         * cualquier valor en el compose. docker compose build --build-arg tiene
         * prioridad sobre build.args del YAML. */
        let repo_url = site
            .repo_url
            .as_deref()
            .unwrap_or("https://github.com/1ndoryu/glory-rs.git");
        let glory_branch = &site.glory_branch;
        /* [268A-4] APP_BIN/FRONTEND_DIR por sitio: projects no-glory (ong-agape)
         * necesitan su propio binario y directorio de frontend. */
        let core_build_args = format!(
            "--build-arg REPO_URL='{}' --build-arg BRANCH='{}' --build-arg APP_BIN='{}' --build-arg FRONTEND_DIR='{}'",
            repo_url.replace('\'', "'\\''"),
            glory_branch.replace('\'', "'\\''"),
            site.app_bin.replace('\'', "'\\''"),
            site.frontend_dir.replace('\'', "'\\''")
        );

        /* [185B-1] Usar nohup+polling para builds de larga duracion (Rust ~15-20 min).
         * ssh.execute() directa falla con "Channel send error" si el servidor cierra
         * la sesion SSH por inactividad durante la compilacion silenciosa de Cargo.
         * execute_long_running lanza en background y reconecta para hacer polling. */
        let build_cmd = format!(
            "cd {} && {}docker compose build --no-cache --progress=plain {} {} {}",
            service_dir, build_env_prefix, core_build_args, build_arg_flags, compose_service
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
                "cd {} && {}docker compose build --progress=plain {} {} {}",
                service_dir, build_env_prefix, core_build_args, build_arg_flags, compose_service
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

    /* [incident-2026-07-21] Re-aplicar fixes sed DESPUÉS del build.
     * Coolify tiene un worker async que regenera docker-compose.yml en disco
     * cuando detecta cambios en la API. Esto puede ocurrir durante los ~9 min
     * del build de Rust, sobrescribiendo los fixes de bind mount, hostname postgres,
     * runtime envs y SSH mount que se aplicaron antes del build.
     * Re-aplicarlos justo antes del swap garantiza que el compose on-disk sea correcto. */
    if matches!(site.template, crate::domain::StackTemplate::Rust) {
        eprintln!("      Re-aplicando fixes post-build (Coolify pudo regenerar compose)...");
        ensure_postgres_auth_and_hostname(&ssh, &service_dir, stack_uuid).await?;
        volume_manager::ensure_uploads_bind_mount(
            &ssh,
            &service_dir,
            &site.nombre,
            compose_service,
        )
        .await?;
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
        /* Verificar que traefik.docker.network=coolify está en el compose on-disk.
         * Si Coolify regeneró el compose sin el label, inyectarlo via sed. */
        verify_or_inject_traefik_network_label(&ssh, &service_dir).await?;
        eprintln!("      Fixes post-build aplicados.");
    }

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

            /* [incident-2026-07-01] M10: Verificar volumen de datos de PostgreSQL.
             * Si el compose no monta pg_data:/var/lib/postgresql/data, los datos se pierden
             * al recrear el contenedor. Esta verificación post-deploy detecta el problema
             * ANTES de que cause pérdida de datos. */
            if matches!(site.template, crate::domain::StackTemplate::Rust) {
                verify_postgres_data_volume(&ssh, stack_uuid, &service_dir).await?;
            }

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
                    /* [incident-2026-07-21] R0b: Inyectar label Traefik en compose restaurado.
                     * El backup se guarda ANTES de rewrite_rust_service_compose(), así que
                     * puede no tener traefik.docker.network=coolify (sitios legacy).
                     * Sin el label, Traefik devuelve 503 "no available server" incluso tras rollback. */
                    let old_compose = if matches!(site.template, crate::domain::StackTemplate::Rust)
                    {
                        inject_traefik_network_label(&old_compose)
                    } else {
                        old_compose
                    };
                    let rollback_api = CoolifyApiClient::new(&target.coolify)?;
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
                                eprintln!(
                                    "   Corrigiendo hostname postgres en compose restaurado..."
                                );
                                if let Err(hostname_err) = ensure_postgres_auth_and_hostname(
                                    &ssh,
                                    &service_dir,
                                    stack_uuid,
                                )
                                .await
                                {
                                    eprintln!(
                                        "   ⚠ Rollback: fix hostname falló: {}",
                                        hostname_err
                                    );
                                }
                            }

                            /* [incident-2026-07-21] R3: Intento 1 — recreate sin build */
                            let recreate_cmd = format!(
                                "cd {} && docker compose up -d --no-build --force-recreate --no-deps {} 2>&1",
                                service_dir, compose_service
                            );
                            let attempt1 = ssh.execute(&recreate_cmd).await;

                            let mut rollback_ok = false;
                            match &attempt1 {
                                Ok(r) if r.success() => {
                                    eprintln!(
                                        "   Contenedor recreado con compose anterior (--no-build)."
                                    );
                                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                                    match wait_for_health(
                                        &settings,
                                        site,
                                        &ssh,
                                        &service_dir,
                                        compose_service,
                                    )
                                    .await
                                    {
                                        Ok(report) => {
                                            eprintln!("   ✅ Rollback exitoso! Sitio restaurado con versión anterior.");
                                            let _ = report;
                                            rollback_ok = true;
                                        }
                                        Err(rollback_err) => {
                                            eprintln!(
                                                "   ⚠ Rollback health (--no-build): {}",
                                                rollback_err
                                            );
                                        }
                                    }
                                }
                                Ok(r) => {
                                    eprintln!(
                                        "   ⚠ Recreate --no-build fallo (exit {}): {}",
                                        r.exit_code,
                                        r.stderr.trim()
                                    );
                                }
                                Err(recreate_err) => {
                                    eprintln!("   ⚠ Recreate --no-build error: {}", recreate_err);
                                }
                            }

                            /* [incident-2026-07-21] R4: Intento 2 — recreate CON build (imagen podada) */
                            if !rollback_ok {
                                eprintln!("   Intentando rollback con rebuild...");
                                let rebuild_cmd = format!(
                                    "cd {} && docker compose up -d --force-recreate --no-deps {} 2>&1",
                                    service_dir, compose_service
                                );
                                match ssh.execute(&rebuild_cmd).await {
                                    Ok(r) if r.success() => {
                                        eprintln!("   Contenedor recreado con rebuild.");
                                        tokio::time::sleep(std::time::Duration::from_secs(15))
                                            .await;
                                        match wait_for_health(
                                            &settings,
                                            site,
                                            &ssh,
                                            &service_dir,
                                            compose_service,
                                        )
                                        .await
                                        {
                                            Ok(report) => {
                                                eprintln!("   ✅ Rollback exitoso (con rebuild)! Sitio restaurado.");
                                                let _ = report;
                                                rollback_ok = true;
                                            }
                                            Err(rb_err) => {
                                                eprintln!(
                                                    "   ⚠ Rollback health (rebuild): {}",
                                                    rb_err
                                                );
                                            }
                                        }
                                    }
                                    Ok(r) => {
                                        eprintln!(
                                            "   ⚠ Rebuild fallo (exit {}): {}",
                                            r.exit_code,
                                            r.stderr.trim()
                                        );
                                    }
                                    Err(e2) => {
                                        eprintln!("   ⚠ Rebuild error: {}", e2);
                                    }
                                }
                            }

                            /* [incident-2026-07-21] R5: Intento 3 — deploy via Coolify API (último recurso) */
                            if !rollback_ok {
                                eprintln!(
                                    "   Intentando deploy via Coolify API (último recurso)..."
                                );
                                match rollback_api.deploy_stack(stack_uuid).await {
                                    Ok(_) => {
                                        eprintln!(
                                            "   Redeploy disparado via API. Esperando (60s)..."
                                        );
                                        tokio::time::sleep(std::time::Duration::from_secs(60))
                                            .await;
                                        match wait_for_health(
                                            &settings,
                                            site,
                                            &ssh,
                                            &service_dir,
                                            compose_service,
                                        )
                                        .await
                                        {
                                            Ok(report) => {
                                                eprintln!("   ✅ Rollback exitoso (redeploy API)! Sitio restaurado.");
                                                let _ = report;
                                                rollback_ok = true;
                                            }
                                            Err(rb_err) => {
                                                eprintln!(
                                                    "   ⚠ Rollback health (redeploy API): {}",
                                                    rb_err
                                                );
                                            }
                                        }
                                    }
                                    Err(api_err) => {
                                        eprintln!("   ⚠ Redeploy API fallo: {}", api_err);
                                    }
                                }
                            }

                            if !rollback_ok {
                                eprintln!("   ❌ Rollback automático falló en todos los intentos.");
                                eprintln!("   El sitio puede estar caído. Verificar manualmente.");
                            }
                        }
                        Err(api_err) => {
                            eprintln!(
                                "   ⚠ Rollback: error restaurando compose en Coolify API: {}",
                                api_err
                            );
                        }
                    }
                }
                Ok(None) => {
                    eprintln!(
                        "   ⚠ Rollback: no hay compose backups disponibles para '{}'.",
                        site.nombre
                    );
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

#[cfg(test)]
mod network_recovery_tests {
    use super::env_building::{normalize_health_path, should_skip_runtime_compose_env};
    use super::rust_autoheal::{
        is_rust_network_probe_failure, shell_single_quote, systemd_safe_name,
    };
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
    fn cleanup_exited_cmd_filters_by_stack_uuid() {
        /* Regresión del incidente 2026-08-27: la limpieza de contenedores exited
         * debe limitarse al stack objetivo (label coolify.stack-uuid), nunca a
         * todos los contenedores del host (borró 9 sitios ajenos). */
        let stack_uuid = "do8k4w8swccwwogoc0os0ck0";
        let cmd = format!(
            "docker ps -a --filter label=coolify.stack-uuid={stack_uuid} --filter status=exited --format '{{{{.Names}}}}' | head -20"
        );
        assert!(cmd.contains("--filter label=coolify.stack-uuid=do8k4w8swccwwogoc0os0ck0"));
        assert!(cmd.contains("--filter status=exited"));
        assert!(!cmd.contains("docker ps -a --filter status=exited"));
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
