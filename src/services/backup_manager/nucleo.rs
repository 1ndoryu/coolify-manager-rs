/* Split 119A-5 de backup_manager.rs — API publica + orquestacion local/restore.
 * Re-exportado sin cambios desde backup_manager/mod.rs (ruta externa intacta). */

use super::ayudantes::{
    archive_container_path, build_artifact, build_backup_id, build_remote_client, cleanup_dir,
    create_backup_archive, export_database_binding, materialize_remote_backup, prune_retention,
    read_manifest, resolve_backup_root, restore_archive_to_container, restore_database_artifact,
    sanitize_path_name, validate_backup_dir,
};
use super::servidor::create_site_backup_server_side;
use super::tipos::{
    BackupExecutionOptions, BackupManifest, BackupStatus, DriveBackupEntry, RemoteClient,
    SiteBackupEntries, SiteBackupListFailure, SiteBackupListReport, MANIFEST_FILE,
};
use crate::config::Settings;
use crate::domain::{BackupTier, SiteConfig};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::{health_manager, site_capabilities};

use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};

pub async fn create_site_backup(
    settings: &Settings,
    config_path: &Path,
    site: &SiteConfig,
    ssh: &SshClient,
    tier: BackupTier,
    label: Option<&str>,
) -> std::result::Result<BackupManifest, CoolifyError> {
    create_site_backup_with_options(
        settings,
        config_path,
        site,
        ssh,
        tier,
        label,
        &BackupExecutionOptions::default(),
    )
    .await
}

/* [245A-9] create_site_backup_with_options ya era el entrypoint legacy del motor.
 * Este bloque solo lo reutiliza desde lightweight mediante wrappers publicos. */
// sentinel-disable-next-line limite-lineas
pub async fn create_site_backup_with_options(
    settings: &Settings,
    config_path: &Path,
    site: &SiteConfig,
    ssh: &SshClient,
    tier: BackupTier,
    label: Option<&str>,
    _options: &BackupExecutionOptions,
) -> std::result::Result<BackupManifest, CoolifyError> {
    if !site.backup_policy.enabled {
        return Err(CoolifyError::Validation(format!(
            "Backups deshabilitados para '{}'",
            site.nombre
        )));
    }

    /* Validar almacenamiento remoto accesible ANTES de crear archivos locales */
    let remote_client = build_remote_client(settings, config_path).await?;
    remote_client.ensure_writable().await?;

    /* [DIRECT-TRANSFER] Si el backend es SSH con directTransferKey configurado,
     * todo el backup se crea en VPS1 y se transfiere a VPS2 directamente.
     * Esto evita transferir 400+ MB por internet domestico (35 min → 2 min). */
    if remote_client.supports_direct_transfer() {
        return create_site_backup_server_side(
            settings,
            config_path,
            site,
            ssh,
            tier,
            label,
            &remote_client,
        )
        .await;
    }

    let backup_id = build_backup_id(label);
    let backup_root = resolve_backup_root(settings, config_path);
    /* [119A-5] canonicalize: backup_id = timestamp + label sanitizado (alnum). */
    validation::validar_segmento_ruta(&backup_id, "backup")?;
    let staging_name = format!(".staging-{backup_id}");
    validation::validar_segmento_ruta(&staging_name, "backup")?;
    let staging_dir = validation::join_segmento_seguro(&backup_root, &staging_name, "backup")?;
    fs::create_dir_all(&staging_dir)?;

    let mut manifest = BackupManifest {
        backup_id: backup_id.clone(),
        site_name: site.nombre.clone(),
        tier: tier.clone(),
        status: BackupStatus::Creating,
        created_at: Utc::now(),
        label: label.map(|value| value.to_string()),
        artifacts: Vec::new(),
        notes: Vec::new(),
    };

    let backup_result =
        collect_local_backup_artifacts(settings, site, ssh, &staging_dir, &mut manifest).await;

    if let Err(error) = backup_result {
        manifest.status = BackupStatus::Failed;
        manifest.notes.push(error.to_string());
        let _ = cleanup_dir(&staging_dir);
        return Err(error);
    }

    /* Crear archive tar.gz empaquetando todo el staging */
    /* [119A-5] canonicalize delegado en join_segmento_seguro. */
    let archive_name = format!("{backup_id}.tar.gz");
    validation::validar_segmento_ruta(&archive_name, "backup")?;
    let archive_path = validation::join_segmento_seguro(&backup_root, &archive_name, "backup")?;
    create_backup_archive(&staging_dir, &archive_path)?;

    /* Subir al almacenamiento remoto */
    let upload_result = remote_client
        .upload(&site.nombre, &tier.to_string(), &backup_id, &archive_path)
        .await;

    /* Limpiar staging y archive local siempre */
    let _ = cleanup_dir(&staging_dir);
    let _ = fs::remove_file(&archive_path);

    match upload_result {
        Ok(file_id) => {
            manifest.status = BackupStatus::Ready;
            manifest.notes.push(format!(
                "remote.{}.id={file_id}",
                remote_client.backend_name()
            ));
            println!(
                "Backup '{}' subido a {} (id: {file_id})",
                backup_id,
                remote_client.backend_name()
            );

            /* Podar backups antiguos segun la politica de retencion */
            if let Err(prune_error) = prune_retention(&remote_client, site, &tier).await {
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

async fn collect_local_backup_artifacts(
    settings: &Settings,
    site: &SiteConfig,
    ssh: &SshClient,
    staging_dir: &Path,
    manifest: &mut BackupManifest,
) -> std::result::Result<(), CoolifyError> {
    let caps = site_capabilities::resolve(site);
    let source_paths = caps.persistent_paths.clone();
    let stack_uuid = site.stack_uuid.as_deref().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{}' sin stackUuid", site.nombre))
    })?;
    let app_container = caps.resolve_app_container(ssh, stack_uuid).await?;

    for binding in &caps.database_bindings {
        let db_container = caps
            .resolve_database_container(ssh, stack_uuid, binding)
            .await?;
        /* [119A-5] canonicalize: logical_name de capabilities internas, validado. */
        let db_name = format!("db-{}.sql", binding.logical_name);
        validation::validar_segmento_ruta(&db_name, "backup")?;
        let output_file = validation::join_segmento_seguro(staging_dir, &db_name, "backup")?;
        export_database_binding(
            settings,
            site,
            ssh,
            &app_container,
            &db_container,
            binding.engine.clone(),
            binding.logical_name,
            &output_file,
        )
        .await?;
        manifest.artifacts.push(build_artifact(
            "database",
            binding.logical_name,
            &output_file,
            None,
        )?);
    }

    for source_path in &source_paths {
        let safe_name = sanitize_path_name(source_path);
        /* [119A-5] canonicalize: safe_name ya sanitizado (alnum), validado. */
        let files_name = format!("files-{}.tar.gz", safe_name);
        validation::validar_segmento_ruta(&files_name, "backup")?;
        let archive_path = validation::join_segmento_seguro(staging_dir, &files_name, "backup")?;
        archive_container_path(ssh, &app_container, source_path, &archive_path).await?;
        manifest.artifacts.push(build_artifact(
            "files",
            &safe_name,
            &archive_path,
            Some(source_path.clone()),
        )?);
    }

    validate_backup_dir(staging_dir, manifest)?;
    Ok(())
}

pub async fn list_site_backups(
    settings: &Settings,
    config_path: &Path,
    site_name: &str,
) -> std::result::Result<Vec<DriveBackupEntry>, CoolifyError> {
    let remote_client = build_remote_client(settings, config_path).await?;
    list_site_backups_with_client(&remote_client, site_name).await
}

pub async fn upload_site_backup_archive(
    settings: &Settings,
    config_path: &Path,
    site_name: &str,
    tier: &BackupTier,
    backup_id: &str,
    local_path: &Path,
) -> std::result::Result<String, CoolifyError> {
    let remote_client = build_remote_client(settings, config_path).await?;
    remote_client.ensure_writable().await?;
    remote_client
        .upload(site_name, &tier.to_string(), backup_id, local_path)
        .await
}

pub async fn prune_site_backup_retention(
    settings: &Settings,
    config_path: &Path,
    site_name: &str,
    tier: &BackupTier,
    keep: usize,
) -> std::result::Result<(), CoolifyError> {
    if matches!(tier, BackupTier::Manual) {
        return Ok(());
    }

    let remote_client = build_remote_client(settings, config_path).await?;
    let tier_name = tier.to_string();
    let files = remote_client.list_tier_files(site_name, &tier_name).await?;

    for (file_id, _) in files.into_iter().skip(keep) {
        remote_client.delete_file(&file_id).await?;
    }

    Ok(())
}

pub async fn materialize_site_backup(
    settings: &Settings,
    config_path: &Path,
    site_name: &str,
    backup_id: &str,
) -> std::result::Result<Option<PathBuf>, CoolifyError> {
    materialize_remote_backup(settings, config_path, site_name, backup_id).await
}

async fn list_site_backups_with_client(
    remote_client: &RemoteClient,
    site_name: &str,
) -> std::result::Result<Vec<DriveBackupEntry>, CoolifyError> {
    let mut entries = Vec::new();

    for tier in [BackupTier::Daily, BackupTier::Weekly, BackupTier::Manual] {
        let tier_name = tier.to_string();
        let files = remote_client.list_tier_files(site_name, &tier_name).await?;
        for (file_id, name) in files {
            let backup_id = name.strip_suffix(".tar.gz").unwrap_or(&name).to_string();
            entries.push(DriveBackupEntry {
                backup_id,
                tier: tier.clone(),
                file_id,
                file_name: name,
            });
        }
    }

    Ok(entries)
}

pub async fn list_all_site_backups(
    settings: &Settings,
    config_path: &Path,
    sites: &[SiteConfig],
) -> std::result::Result<SiteBackupListReport, CoolifyError> {
    /* [105A-28] El listado global reutiliza un unico cliente remoto.
     * Gotcha: construir cliente por sitio abre/revalida remoto muchas veces y hacia lenta la tabla. */
    let remote_client = build_remote_client(settings, config_path).await?;
    let mut report = SiteBackupListReport {
        sites: Vec::new(),
        errors: Vec::new(),
    };

    for site in sites {
        match list_site_backups_with_client(&remote_client, &site.nombre).await {
            Ok(entries) => report.sites.push(SiteBackupEntries {
                site_name: site.nombre.clone(),
                entries,
            }),
            Err(error) => report.errors.push(SiteBackupListFailure {
                site_name: site.nombre.clone(),
                message: format!("{error:#}"),
            }),
        }
    }

    Ok(report)
}

/* [245A-9] restore_site_backup sigue compartido entre motores legacy.
 * Separarlo queda fuera del corte funcional de backup/restore lightweight. */
// sentinel-disable-next-line limite-lineas
pub async fn restore_site_backup(
    settings: &Settings,
    config_path: &Path,
    site: &SiteConfig,
    ssh: &SshClient,
    backup_id: &str,
    skip_safety_snapshot: bool,
) -> std::result::Result<(), CoolifyError> {
    let (manifest_dir, manifest) =
        load_materialized_backup_manifest(settings, config_path, &site.nombre, backup_id).await?;

    if manifest.status != BackupStatus::Ready {
        return Err(CoolifyError::Validation(format!(
            "Backup '{}' no esta listo para restaurar",
            backup_id
        )));
    }

    let safety_backup = if skip_safety_snapshot {
        None
    } else {
        Some(
            create_site_backup(
                settings,
                config_path,
                site,
                ssh,
                BackupTier::Manual,
                Some("pre-restore"),
            )
            .await?,
        )
    };

    let restore_result =
        restore_materialized_backup(settings, site, ssh, &manifest_dir, &manifest).await;

    match (restore_result, safety_backup) {
        (Ok(_), _) => Ok(()),
        (Err(error), Some(safety)) => {
            tracing::error!(
                "Restore fallo, intentando rollback con backup de seguridad {}",
                safety.backup_id
            );
            let rollback_result = Box::pin(restore_site_backup(
                settings,
                config_path,
                site,
                ssh,
                &safety.backup_id,
                true,
            ))
            .await;
            if let Err(rollback_error) = rollback_result {
                return Err(CoolifyError::Validation(format!(
                    "Restore fallo y rollback tambien fallo: {} | rollback: {}",
                    error, rollback_error
                )));
            }
            Err(error)
        }
        (Err(error), None) => Err(error),
    }
}

async fn load_materialized_backup_manifest(
    settings: &Settings,
    config_path: &Path,
    site_name: &str,
    backup_id: &str,
) -> std::result::Result<(PathBuf, BackupManifest), CoolifyError> {
    let manifest_dir = materialize_remote_backup(settings, config_path, site_name, backup_id)
        .await?
        .ok_or_else(|| {
            CoolifyError::Validation(format!(
                "Backup '{}' no encontrado en almacenamiento remoto para '{}'",
                backup_id, site_name
            ))
        })?;
    let manifest = read_manifest(&manifest_dir.join(MANIFEST_FILE))?;
    validate_backup_dir(&manifest_dir, &manifest)?;
    Ok((manifest_dir, manifest))
}

async fn restore_materialized_backup(
    settings: &Settings,
    site: &SiteConfig,
    ssh: &SshClient,
    manifest_dir: &Path,
    manifest: &BackupManifest,
) -> std::result::Result<(), CoolifyError> {
    let caps = site_capabilities::resolve(site);
    let stack_uuid = site.stack_uuid.as_deref().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{}' sin stackUuid", site.nombre))
    })?;
    let app_container = caps.resolve_app_container(ssh, stack_uuid).await?;

    for artifact in &manifest.artifacts {
        /* [119A-5] canonicalize: relative_path validado (sin .. ni absoluto). */
        let local_path =
            validation::unir_relativo_seguro(manifest_dir, &artifact.relative_path, "backup")?;
        match artifact.kind.as_str() {
            "database" => {
                restore_database_artifact(
                    settings,
                    site,
                    ssh,
                    &app_container,
                    &caps,
                    stack_uuid,
                    artifact,
                    &local_path,
                )
                .await?;
            }
            "files" => {
                let target_path = artifact.original_path.as_deref().ok_or_else(|| {
                    CoolifyError::Validation(format!(
                        "Artifacto '{}' sin original_path",
                        artifact.relative_path
                    ))
                })?;
                restore_archive_to_container(ssh, &app_container, &local_path, target_path).await?;
            }
            other => {
                return Err(CoolifyError::Validation(format!(
                    "Tipo de artifacto desconocido: {other}"
                )));
            }
        }
    }

    health_manager::assert_site_healthy(settings, site, ssh).await?;
    Ok(())
}
