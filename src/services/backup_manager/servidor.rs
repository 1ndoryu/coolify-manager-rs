/* Split 119A-5 de backup_manager.rs — flujo server-side (direct-transfer VPS1→VPS2).
 * Codigo verbatim del original (lineas 794-1153); solo cambia visibilidad a pub(super). */

use super::ayudantes::{build_backup_id, build_remote_artifact};
use super::tipos::{BackupArtifact, BackupManifest, BackupStatus, RemoteClient};
use crate::config::Settings;
use crate::domain::{BackupTier, DatabaseEngine, SiteConfig};
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;
use crate::services::{database_manager, site_capabilities};

use chrono::Utc;
use std::path::Path;

/* ==========================================================================
 * [DIRECT-TRANSFER] Flujo de backup server-side.
 * Todo el staging, compresion y transferencia ocurre en VPS1→VPS2
 * sin pasar datos por el PC local. Speedup tipico: 35 min → 2 min.
 *
 * Flujo:
 * 1. Crear staging dir en VPS1:/tmp/
 * 2. Exportar DBs al staging en VPS1 (nice/ionice para no afectar sitios)
 * 3. Archivar filesystems al staging en VPS1
 * 4. Computar hashes (sha256sum) en VPS1
 * 5. Escribir manifest.json en VPS1
 * 6. Crear tar.gz final en VPS1
 * 7. SCP directo VPS1→VPS2 (~100 Mbps datacenter)
 * 8. Cleanup VPS1
 * ========================================================================== */

#[allow(clippy::too_many_arguments)]
/* [245A-9] La ruta server-side es deuda previa del motor de backups.
 * Se mantiene intacta para no mezclar un refactor grande con este bloque. */
// sentinel-disable-next-line limite-lineas
pub(super) async fn create_site_backup_server_side(
    settings: &Settings,
    _config_path: &Path,
    site: &SiteConfig,
    ssh: &SshClient,
    tier: BackupTier,
    label: Option<&str>,
    remote_client: &RemoteClient,
) -> std::result::Result<BackupManifest, CoolifyError> {
    let ssh_remote = remote_client.as_ssh_remote().ok_or_else(|| {
        CoolifyError::Validation("direct transfer requiere backend SSH".to_string())
    })?;

    let backup_id = build_backup_id(label);
    let staging_dir = format!("/tmp/cm-staging-{backup_id}");
    tracing::info!(
        "Backup server-side '{}' para '{}' (staging: VPS1:{})",
        backup_id,
        site.nombre,
        staging_dir
    );
    create_remote_staging_dir(ssh, &staging_dir).await?;
    let artifacts = match collect_server_side_artifacts(settings, site, ssh, &staging_dir).await {
        Ok(artifacts) => artifacts,
        Err(error) => {
            let _ = ssh.execute(&format!("rm -rf '{staging_dir}'")).await;
            return Err(error);
        }
    };

    /* Fase 3: Crear manifest.json en VPS1 — status Ready porque todos los artifacts ya existen y tienen sha256 */
    let mut manifest = BackupManifest {
        backup_id: backup_id.clone(),
        site_name: site.nombre.clone(),
        tier: tier.clone(),
        status: BackupStatus::Ready,
        created_at: Utc::now(),
        label: label.map(|value| value.to_string()),
        artifacts,
        notes: Vec::new(),
    };

    let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|error| {
        CoolifyError::Validation(format!("No se pudo serializar manifiesto: {error}"))
    })?;
    if let Err(error) = write_manifest_to_remote_staging(ssh, &staging_dir, &manifest_json).await {
        let _ = ssh.execute(&format!("rm -rf '{staging_dir}'")).await;
        return Err(error);
    }

    let archive_path = package_server_side_archive(ssh, &backup_id, &staging_dir).await?;

    /* Fase 5: Transferir VPS1→VPS2 directamente */
    let upload_result = ssh_remote
        .upload_from_vps1(
            ssh,
            &archive_path,
            &site.nombre,
            &tier.to_string(),
            &backup_id,
        )
        .await;

    /* Fase 6: Cleanup VPS1 */
    let _ = ssh.execute(&format!("rm -f '{archive_path}'")).await;

    match upload_result {
        Ok(file_id) => {
            manifest.status = BackupStatus::Ready;
            manifest.notes.push(format!(
                "remote.{}.id={file_id}",
                remote_client.backend_name()
            ));
            manifest
                .notes
                .push("transfer.mode=direct-vps1-to-vps2".to_string());
            println!(
                "Backup '{}' transferido VPS1→VPS2 (id: {file_id})",
                backup_id,
            );

            if let Err(prune_error) =
                super::ayudantes::prune_retention(remote_client, site, &tier).await
            {
                tracing::warn!("No se pudo podar backups antiguos: {prune_error}");
            }

            Ok(manifest)
        }
        Err(error) => {
            manifest.status = BackupStatus::Failed;
            manifest.notes.push(format!("upload.error={error}"));
            Err(error)
        }
    }
}

async fn create_remote_staging_dir(
    ssh: &SshClient,
    staging_dir: &str,
) -> std::result::Result<(), CoolifyError> {
    let result = ssh.execute(&format!("mkdir -p '{staging_dir}'")).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo crear staging en VPS1: {}",
            result.stderr
        )));
    }
    Ok(())
}

async fn collect_server_side_artifacts(
    _settings: &Settings,
    site: &SiteConfig,
    ssh: &SshClient,
    staging_dir: &str,
) -> std::result::Result<Vec<BackupArtifact>, CoolifyError> {
    let caps = site_capabilities::resolve(site);
    let source_paths = caps.persistent_paths.clone();
    let stack_uuid = site.stack_uuid.as_deref().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{}' sin stackUuid", site.nombre))
    })?;
    let app_container = caps.resolve_app_container(ssh, stack_uuid).await?;
    let mut artifacts = Vec::new();

    for binding in &caps.database_bindings {
        let db_container = caps
            .resolve_database_container(ssh, stack_uuid, binding)
            .await?;
        let host_output = format!("{}/db-{}.sql", staging_dir, binding.logical_name);
        export_database_binding_to_host(
            ssh,
            &app_container,
            &db_container,
            binding.engine.clone(),
            &host_output,
        )
        .await?;
        artifacts.push(
            build_remote_artifact(ssh, "database", binding.logical_name, &host_output, None)
                .await?,
        );
    }

    for source_path in &source_paths {
        let safe_name = super::ayudantes::sanitize_path_name(source_path);
        let host_output = format!("{}/files-{}.tar.gz", staging_dir, safe_name);
        archive_container_path_to_host(ssh, &app_container, source_path, &host_output).await?;
        artifacts.push(
            build_remote_artifact(
                ssh,
                "files",
                &safe_name,
                &host_output,
                Some(source_path.clone()),
            )
            .await?,
        );
    }

    Ok(artifacts)
}

async fn write_manifest_to_remote_staging(
    ssh: &SshClient,
    staging_dir: &str,
    manifest_json: &str,
) -> std::result::Result<(), CoolifyError> {
    let write_cmd = format!(
        "cat > '{staging_dir}/manifest.json' << 'CM_MANIFEST_EOF'\n{manifest_json}\nCM_MANIFEST_EOF"
    );
    let result = ssh.execute(&write_cmd).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo escribir manifest en VPS1: {}",
            result.stderr
        )));
    }
    Ok(())
}

async fn package_server_side_archive(
    ssh: &SshClient,
    backup_id: &str,
    staging_dir: &str,
) -> std::result::Result<String, CoolifyError> {
    let staging_name = format!(".staging-{backup_id}");
    let archive_path = format!("/tmp/cm-backup-{backup_id}.tar.gz");
    let renamed = format!("/tmp/{staging_name}");
    ssh.execute(&format!("mv '{staging_dir}' '{renamed}'"))
        .await?;

    let tar_result = ssh
        .execute(&format!(
            "cd /tmp && tar -czf '{archive_path}' '{staging_name}'"
        ))
        .await?;
    if !tar_result.success() {
        let _ = ssh
            .execute(&format!("rm -rf '{renamed}' '{archive_path}'"))
            .await;
        return Err(CoolifyError::Validation(format!(
            "No se pudo crear archive en VPS1: {}",
            tar_result.stderr
        )));
    }

    let _ = ssh.execute(&format!("rm -rf '{renamed}'")).await;
    Ok(archive_path)
}

/* Exporta base de datos dejando el SQL en VPS1 (server-side). */
async fn export_database_binding_to_host(
    ssh: &SshClient,
    app_container: &str,
    db_container: &str,
    engine: DatabaseEngine,
    host_output: &str,
) -> std::result::Result<(), CoolifyError> {
    match engine {
        DatabaseEngine::Mariadb => {
            let (db_name, db_user, db_password) =
                database_manager::resolve_wordpress_credentials(ssh, app_container).await?;
            database_manager::export_database_to_host(
                ssh,
                db_container,
                &db_name,
                &db_user,
                &db_password,
                host_output,
            )
            .await
        }
        DatabaseEngine::Postgres => {
            let (db_name, db_user, db_password) =
                database_manager::resolve_postgres_credentials(ssh, app_container).await?;
            database_manager::export_postgres_database_to_host(
                ssh,
                db_container,
                &db_name,
                &db_user,
                &db_password,
                host_output,
            )
            .await
        }
    }
}

/* Archiva un path del contenedor dejando el tar.gz en VPS1 host (server-side).
 * Usa nice/ionice para minimizar impacto en el contenedor en produccion. */
async fn archive_container_path_to_host(
    ssh: &SshClient,
    container_id: &str,
    source_path: &str,
    host_output: &str,
) -> std::result::Result<(), CoolifyError> {
    let container_archive = format!(
        "/tmp/cm_backup_{}.tar.gz",
        super::ayudantes::sanitize_path_name(source_path)
    );
    let stripped = source_path.trim_start_matches('/');

    /* nice -n 19 + ionice -c 3 = minima prioridad CPU/IO para no afectar trafico del sitio */
    let command = format!(
        "test -e {path} && nice -n 19 ionice -c 3 tar --warning=no-file-changed -czf {archive} -C / {stripped}",
        path = source_path,
        archive = container_archive,
        stripped = stripped,
    );
    let result = docker::docker_exec(ssh, container_id, &command).await?;
    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!(
                "No se pudo empaquetar '{source_path}' (server-side): {}",
                result.stderr
            ),
        });
    }

    /* docker cp del contenedor al host VPS1 (I/O local, instantaneo) */
    docker::copy_from_container_to_host(ssh, container_id, &container_archive, host_output).await?;
    let _ = docker::docker_exec(ssh, container_id, &format!("rm -f {container_archive}")).await;
    Ok(())
}
