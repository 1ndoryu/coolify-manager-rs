/* Fases núcleo del deploy: build, swap, traefik, salud y seed (119A-5 split deploy_service). */

use super::contexto::CtxDeploy;
use super::rollback::intentar_rollback;
use super::*;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

/* [3/6] Build de la imagen nueva + fixes post-build (Coolify pudo regenerar
 * el compose on-disk durante el build). */
pub(super) async fn fase_build(
    ctx: &CtxDeploy<'_>,
    ssh: &mut SshClient,
    runtime_envs: &[(String, String)],
) -> std::result::Result<(), CoolifyError> {
    let site = ctx.site;
    let target = &ctx.target;
    let service_dir = &ctx.service_dir;
    let compose_service = ctx.compose_service.as_str();
    let stack_uuid = ctx.stack_uuid;
    let skip_build = ctx.skip_build;

    /* [119A-4] Modo imagen: el sitio fija imageRef (p. ej. ghcr.io/1ndoryu/app:sha)
     * y la VPS solo descarga la imagen compilada fuera (GitHub Actions).
     * No hay build en la VPS: el build Rust de ~10 min al 100% CPU provocó
     * el reinicio de dockerd del 2026-09-20 con 11 sitios caídos.
     * Rollback operativo: fijar el tag anterior en imageRef y re-deployar
     * (la imagen previa sigue en caché local → swap en segundos). */
    if let Some(image_ref) = site.image_ref.as_deref() {
        validation::validate_image_ref(image_ref)?;
        println!("[3/6] Descargando imagen precompilada (sin build en VPS)...");
        println!("      Imagen: {image_ref}");
        let pull_start = std::time::Instant::now();
        let pull_cmd = format!("cd {service_dir} && docker compose pull {compose_service}");
        let pull_result = ssh.execute(&pull_cmd).await?;
        if !pull_result.success() {
            return Err(CoolifyError::Validation(format!(
                "Pull de '{image_ref}' fallo:\n{}",
                command_output_summary(&pull_result.stdout, &pull_result.stderr)
            )));
        }
        println!(
            "      Imagen descargada en {}s.",
            pull_start.elapsed().as_secs()
        );
    } else if !skip_build {
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
    let compose_image = ensure_compose_service_image_available(ssh, service_dir, compose_service)
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
        ensure_postgres_auth_and_hostname(ssh, service_dir, stack_uuid).await?;
        volume_manager::ensure_uploads_bind_mount(ssh, service_dir, &site.nombre, compose_service)
            .await?;
        volume_manager::ensure_runtime_envs_in_compose(
            ssh,
            service_dir,
            compose_service,
            runtime_envs,
        )
        .await?;
        volume_manager::ensure_runtime_ssh_bind_mount(
            ssh,
            service_dir,
            compose_service,
            &site.nombre,
        )
        .await?;
        /* Verificar que traefik.docker.network=coolify está en el compose on-disk.
         * Si Coolify regeneró el compose sin el label, inyectarlo via sed. */
        verify_or_inject_traefik_network_label(ssh, service_dir).await?;
        eprintln!("      Fixes post-build aplicados.");
    }
    Ok(())
}

/* [4/6] Swap: reemplazar el contenedor con la imagen nueva. */
pub(super) async fn fase_swap(
    ctx: &CtxDeploy<'_>,
    ssh: &SshClient,
) -> std::result::Result<(), CoolifyError> {
    let site = ctx.site;
    let service_dir = &ctx.service_dir;
    let compose_service = ctx.compose_service.as_str();

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
        ssh,
        service_dir,
        compose_service,
        &site.nombre,
    )
    .await?;
    Ok(())
}

/* [5/6] Conectar Traefik y Coolify interno a la red del servicio. */
pub(super) async fn fase_traefik(
    ctx: &CtxDeploy<'_>,
    ssh: &SshClient,
) -> std::result::Result<(), CoolifyError> {
    let service_dir = &ctx.service_dir;
    let compose_service = ctx.compose_service.as_str();
    let stack_uuid = ctx.stack_uuid;

    println!("[5/6] Verificando conectividad Traefik/Coolify...");
    ensure_traefik_connected(ssh, stack_uuid).await?;
    ensure_app_coolify_network(ssh, service_dir, compose_service).await?;
    println!("      Contenedor reemplazado.");
    Ok(())
}

/* [6/6] Health check + verificaciones post-deploy + autoheal. Si el health
 * falla, intenta rollback automatico antes de devolver el error. */
pub(super) async fn fase_salud(
    ctx: &CtxDeploy<'_>,
    ssh: &SshClient,
) -> std::result::Result<(), CoolifyError> {
    let settings = ctx.settings;
    let site = ctx.site;
    let service_dir = &ctx.service_dir;
    let compose_service = ctx.compose_service.as_str();
    let stack_uuid = ctx.stack_uuid;
    let caps = site_capabilities::resolve(site);

    println!("[6/6] Verificando salud...");
    let health_result = wait_for_health(settings, site, ssh, service_dir, compose_service).await;

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
                verify_container_env_vars(ssh, &site.nombre, service_dir, compose_service).await?;
            }

            /* [04A-1] M9: Post-deploy volume verification.
             * Verifica que los volúmenes nombrados están montados.
             * Resuelve E9 (volúmenes huérfanos sin attach). */
            verify_container_volumes(ssh, &site.nombre, service_dir, compose_service).await?;

            /* [incident-2026-07-01] M10: Verificar volumen de datos de PostgreSQL.
             * Si el compose no monta pg_data:/var/lib/postgresql/data, los datos se pierden
             * al recrear el contenedor. Esta verificación post-deploy detecta el problema
             * ANTES de que cause pérdida de datos. */
            if matches!(site.template, crate::domain::StackTemplate::Rust) {
                verify_postgres_data_volume(ssh, stack_uuid, service_dir).await?;
            }

            if matches!(site.template, crate::domain::StackTemplate::Rust) {
                install_rust_public_autoheal(
                    ssh,
                    site,
                    stack_uuid,
                    service_dir,
                    compose_service,
                    &url,
                )
                .await?;
            }
            Ok(())
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

            /* [119A-2/B0] E11: si el dominio ni siquiera resuelve por DNS
             * (sitio nuevo sin DNS propagado), el health HTTPS falla aunque el
             * contenedor esté healthy. El rollback sería ciego (bucle rebuild
             * ~10 min/ciclo contra una app sana): se omite con warning. */
            let url = caps.health_url(site);
            if !dominio_salud_resuelve(&url) {
                eprintln!(
                    "\n⚠ E11/B0: {} no resuelve por DNS. El contenedor puede estar sano; \
                     se omite el rollback automático. Propaga el DNS y reintenta.",
                    extraer_host_salud(&url)
                );
                return Err(e);
            }

            intentar_rollback(ctx, ssh).await;
            Err(e)
        }
    }
}

/* [F7] Health check de TODOS los sitios en el mismo servidor para detectar daños colaterales. */
pub(super) async fn fase_salud_colateral(
    ctx: &CtxDeploy<'_>,
    ssh: &SshClient,
) -> std::result::Result<(), CoolifyError> {
    let settings = ctx.settings;
    let site_name = ctx.site_name;
    let target = &ctx.target;

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
        match health_manager::run_site_health_check(settings, other_site, ssh).await {
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
    Ok(())
}

/* Seed opcional de datos de prueba. */
pub(super) async fn fase_seed(
    ctx: &CtxDeploy<'_>,
    ssh: &SshClient,
) -> std::result::Result<(), CoolifyError> {
    if !ctx.seed {
        return Ok(());
    }
    let service_dir = &ctx.service_dir;
    let compose_service = ctx.compose_service.as_str();

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
    Ok(())
}
