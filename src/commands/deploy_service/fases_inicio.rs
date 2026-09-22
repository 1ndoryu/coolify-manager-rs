/* Fases iniciales del deploy: backup, sync compose y preparacion del host (119A-5 split deploy_service). */

use super::contexto::CtxDeploy;
use super::*;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

/* [F2] Pre-deploy: safety check + backup automatico para poder revertir. */
pub(super) async fn fase_seguridad_backup(
    settings: &Settings,
    config_path: &Path,
    site_name: &str,
    site: &crate::domain::SiteConfig,
    target: &crate::config::DeploymentTargetConfig,
    skip_backup: bool,
) -> std::result::Result<(), CoolifyError> {
    /* [F2] Safety check: verificar que todos los sitios del servidor existen en Coolify */
    println!("[pre] Verificando estado de sitios en Coolify...");
    validation::pre_deploy_safety_check(settings, site_name).await?;

    /* [F8] Backup automatico pre-deploy para poder revertir si algo sale mal */
    if !skip_backup && site.backup_policy.enabled {
        println!("[pre] Creando backup pre-deploy de '{site_name}'...");
        let mut backup_ssh = SshClient::from_vps(&target.vps);
        backup_ssh.connect().await?;
        match backup_manager::create_site_backup(
            settings,
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
    Ok(())
}

/* [1/6] Sync compose con Coolify API. */
pub(super) async fn fase_sync_compose(
    config_path: &Path,
    site: &crate::domain::SiteConfig,
    stack_uuid: &str,
    target: &crate::config::DeploymentTargetConfig,
    skip_compose_sync: bool,
) -> std::result::Result<(), CoolifyError> {
    if !skip_compose_sync {
        println!("[1/6] Sincronizando compose con Coolify...");
        sync_compose(config_path, site, stack_uuid, &target.coolify).await?;
        println!("      Compose sincronizado.");
    } else {
        println!("[1/6] Sync compose omitido (--skip-compose-sync).");
    }
    Ok(())
}

/* [2/6] SSH + verificacion de dependencias. Devuelve las envs runtime de
 * Coolify porque fase_build las necesita para re-aplicar fixes post-build. */
pub(super) async fn fase_preparar_host(
    ctx: &CtxDeploy<'_>,
    ssh: &SshClient,
) -> std::result::Result<Vec<(String, String)>, CoolifyError> {
    let site = ctx.site;
    let config_path = ctx.config_path;
    let target = &ctx.target;
    let service_dir = &ctx.service_dir;
    let stack_uuid = ctx.stack_uuid;
    let skip_compose_sync = ctx.skip_compose_sync;

    println!("[2/6] Conectando via SSH y verificando dependencias...");

    /* Subir Dockerfile del template al directorio del servicio (si existe) */
    /* [119A-5] canonicalize: Dockerfile.<template> viene del enum StackTemplate (fijo). */
    let dockerfile_name = format!("Dockerfile.{}", site.template);
    validation::validar_segmento_ruta(&dockerfile_name, "template")?;
    let templates_base = config_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("templates");
    let dockerfile_path =
        validation::join_segmento_seguro(&templates_base, &dockerfile_name, "template")?;
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

    verify_postgres(ssh, service_dir).await?;
    println!("      Postgres OK.");

    /* [095A-22] Coolify puede regenerar SERVICE_PASSWORD_POSTGRES sin alterar el
     * rol persistente dentro del volumen pg_data. Antes del swap, alinear el rol
     * y forzar el hostname unico postgres-{uuid}; asi la app nueva no arranca
     * contra otro Postgres ni queda en restart loop por 28P01. */
    ensure_postgres_auth_and_hostname(ssh, service_dir, stack_uuid).await?;

    /* [214A-4] Pre-deploy: verificar memoria y disco disponible antes de construir.
     * Build de imágenes Docker consume mucha RAM y disco (layers, cache).
     * Si no hay suficiente espacio, el build falla a mitad y deja basura.
     * Umbrales: ≥512MB RAM libre, ≥3GB disco libre. */
    check_server_resources(ssh, service_dir).await?;

    preparar_uploads_y_binds(ctx, ssh).await
}

/* Cola de fase_preparar_host: directorio de uploads persistente, fusión de
 * uploads del named volume, bind mounts y envs runtime en el compose. */
async fn preparar_uploads_y_binds(
    ctx: &CtxDeploy<'_>,
    ssh: &SshClient,
) -> std::result::Result<Vec<(String, String)>, CoolifyError> {
    let site = ctx.site;
    let target = &ctx.target;
    let service_dir = &ctx.service_dir;
    let compose_service = ctx.compose_service.as_str();
    let stack_uuid = ctx.stack_uuid;
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
        ssh,
        service_dir,
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
    volume_manager::ensure_uploads_bind_mount(ssh, service_dir, &site.nombre, compose_service)
        .await?;
    let runtime_envs = runtime_envs_from_coolify(&target.coolify, stack_uuid).await?;
    volume_manager::ensure_runtime_envs_in_compose(
        ssh,
        service_dir,
        compose_service,
        &runtime_envs,
    )
    .await?;
    volume_manager::ensure_runtime_ssh_bind_mount(ssh, service_dir, compose_service, &site.nombre)
        .await?;

    Ok(runtime_envs)
}
