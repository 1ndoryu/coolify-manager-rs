/* Split 119A-5 de lightweight_runtime_manager.rs — backup/restore del runtime.
 * Codigo verbatim del original; solo cambia visibilidad de ayudantes compartidos. */

use super::plantillas::{build_restore_script, parse_restore_output, sh_quote};
use super::sitios::{list_lightweight_sites, require_site};
use super::tipos::{
    LightweightBackupEntry, LightweightBackupListReport, LightweightBackupReport,
    LightweightRestoreReport,
};
use crate::config::{DeploymentTargetConfig, Settings};
use crate::domain::BackupTier;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::backup_manager::{self, BackupArtifact, BackupManifest, BackupStatus};

use chrono::Utc;
use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use std::fs;
use std::fs::File;
use std::path::Path;
use tar::Builder;

pub async fn list_lightweight_site_backups(
    settings: &Settings,
    config_path: &Path,
    target: &DeploymentTargetConfig,
    site_name: &str,
) -> std::result::Result<LightweightBackupListReport, CoolifyError> {
    let entries = backup_manager::list_site_backups(settings, config_path, site_name).await?;
    Ok(LightweightBackupListReport {
        target: target.name.clone(),
        target_ip: target.vps.ip.clone(),
        site: site_name.to_string(),
        entries: entries
            .into_iter()
            .map(|entry| LightweightBackupEntry {
                backup_id: entry.backup_id,
                tier: entry.tier.to_string(),
                file_id: entry.file_id,
                file_name: entry.file_name,
            })
            .collect(),
    })
}

/* [245A-9] El backup lightweight debe cerrar el ciclo completo del runtime.
 * No basta con empaquetar /srv/hosting/{site}: el restore tiene que rehacer
 * Caddy/SSH y devolver la password regenerada para resincronizar el panel. */
pub async fn create_lightweight_site_backup(
    settings: &Settings,
    config_path: &Path,
    target: &DeploymentTargetConfig,
    site_name: &str,
    tier: BackupTier,
    label: Option<&str>,
) -> std::result::Result<LightweightBackupReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let site = require_site(&ssh, site_name).await?;
    let backup_id = build_backup_id(label);
    /* [119A-5] canonicalize: backup_id = timestamp + label sanitizado (alnum). */
    validation::validar_segmento_ruta(&backup_id, "backup")?;
    let root_name = format!("cm-light-backup-{backup_id}");
    validation::validar_segmento_ruta(&root_name, "backup")?;
    let local_root = validation::join_segmento_seguro(&std::env::temp_dir(), &root_name, "backup")?;
    let staging_dir = validation::join_segmento_seguro(&local_root, &backup_id, "backup")?;
    fs::create_dir_all(&staging_dir)?;

    let artifact_name = format!("files-{}.tar.gz", sanitize_path_name(&site.project_root));
    validation::validar_segmento_ruta(&artifact_name, "backup")?;
    let local_artifact = validation::join_segmento_seguro(&staging_dir, &artifact_name, "backup")?;
    let archive_name = format!("{backup_id}.tar.gz");
    validation::validar_segmento_ruta(&archive_name, "backup")?;
    let local_archive = validation::join_segmento_seguro(&local_root, &archive_name, "backup")?;
    let remote_archive = format!("/tmp/cm-lightweight-backup-{backup_id}.tar.gz");

    let archive_script = [
        "set -euo pipefail".to_string(),
        format!("site_root={}", sh_quote(&site.project_root)),
        format!("archive={}", sh_quote(&remote_archive)),
        "if [ ! -d \"$site_root\" ]; then echo \"Sitio lightweight inexistente\" >&2; exit 24; fi"
            .to_string(),
        "tar -czf \"$archive\" -C / \"${site_root#/}\"".to_string(),
    ]
    .join("\n");

    let archive_result = ssh
        .execute(&format!("bash -lc {}", sh_quote(&archive_script)))
        .await?;
    if !archive_result.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo creando backup lightweight para '{}': {}",
            site_name, archive_result.stderr
        )));
    }

    let download_result = ssh
        .download_file_streamed(&remote_archive, &local_artifact)
        .await;
    let _ = ssh
        .execute(&format!("rm -f {}", sh_quote(&remote_archive)))
        .await;
    download_result?;

    let manifest = BackupManifest {
        backup_id: backup_id.clone(),
        site_name: site.name.clone(),
        tier: tier.clone(),
        status: BackupStatus::Ready,
        created_at: Utc::now(),
        label: label.map(str::to_string),
        artifacts: vec![build_local_artifact(
            "files",
            &sanitize_path_name(&site.project_root),
            &local_artifact,
            Some(site.project_root.clone()),
        )?],
        notes: vec![
            "runtime=lightweight".to_string(),
            format!("source={}", site.project_root),
        ],
    };

    write_local_manifest(&staging_dir, &manifest)?;
    create_local_archive(&staging_dir, &local_archive)?;

    let upload_result = backup_manager::upload_site_backup_archive(
        settings,
        config_path,
        &site.name,
        &tier,
        &backup_id,
        &local_archive,
    )
    .await;

    let _ = cleanup_dir(&local_root);

    let file_id = upload_result?;
    if let Some(keep) = retention_keep_for(&tier) {
        if let Err(error) = backup_manager::prune_site_backup_retention(
            settings,
            config_path,
            &site.name,
            &tier,
            keep,
        )
        .await
        {
            tracing::warn!(
                "No se pudo podar backups lightweight de '{}': {}",
                site_name,
                error
            );
        }
    }

    Ok(LightweightBackupReport {
        target: target.name.clone(),
        target_ip: target.vps.ip.clone(),
        site: site.name,
        backup_id,
        tier: tier.to_string(),
        status: "ready".to_string(),
        notes: vec![
            format!("remote.id={file_id}"),
            "runtime=lightweight".to_string(),
        ],
    })
}

pub async fn restore_lightweight_site_backup(
    settings: &Settings,
    config_path: &Path,
    target: &DeploymentTargetConfig,
    site_name: &str,
    backup_id: &str,
    access_password: Option<&str>,
    skip_safety_snapshot: bool,
) -> std::result::Result<LightweightRestoreReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let existing_site = list_lightweight_sites(&ssh)
        .await?
        .into_iter()
        .find(|site| site.name == site_name || site.deployment_id == site_name);
    let safety_snapshot = if skip_safety_snapshot || existing_site.is_none() {
        None
    } else {
        Some(
            create_lightweight_site_backup(
                settings,
                config_path,
                target,
                site_name,
                BackupTier::Manual,
                Some("pre-restore"),
            )
            .await?
            .backup_id,
        )
    };

    let manifest_dir =
        backup_manager::materialize_site_backup(settings, config_path, site_name, backup_id)
            .await?
            .ok_or_else(|| {
                CoolifyError::Validation(format!(
                    "Backup '{}' no encontrado para '{}'",
                    backup_id, site_name
                ))
            })?;
    let manifest = read_local_manifest(&manifest_dir.join("manifest.json"))?;
    validate_local_manifest(&manifest_dir, &manifest)?;

    let Some(files_artifact) = manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == "files")
    else {
        let _ = cleanup_dir(manifest_dir.parent().unwrap_or(&manifest_dir));
        return Err(CoolifyError::Validation(format!(
            "Backup '{}' de '{}' no contiene artifacts de archivos",
            backup_id, site_name
        )));
    };

    let local_artifact =
        validation::unir_relativo_seguro(&manifest_dir, &files_artifact.relative_path, "backup")?;
    let remote_artifact = format!("/tmp/cm-lightweight-restore-{backup_id}.tar.gz");
    ssh.upload_file_streamed(&local_artifact, &remote_artifact)
        .await?;

    let restore_script = build_restore_script(site_name, &remote_artifact, access_password);
    let restore_result = ssh
        .execute(&format!("bash -lc {}", sh_quote(&restore_script)))
        .await;
    let _ = ssh
        .execute(&format!("rm -f {}", sh_quote(&remote_artifact)))
        .await;
    let _ = cleanup_dir(manifest_dir.parent().unwrap_or(&manifest_dir));

    let restore_result = restore_result?;
    if !restore_result.success() {
        if let Some(safety_backup_id) = safety_snapshot.as_deref() {
            tracing::error!(
                "Restore lightweight '{}' falló; intentando rollback con {}",
                site_name,
                safety_backup_id
            );
            let _ = Box::pin(restore_lightweight_site_backup(
                settings,
                config_path,
                target,
                site_name,
                safety_backup_id,
                access_password,
                true,
            ))
            .await;
        }
        return Err(CoolifyError::Validation(format!(
            "Fallo restaurando backup '{}' de '{}': {}",
            backup_id, site_name, restore_result.stderr
        )));
    }

    let output = parse_restore_output(&restore_result.stdout);
    let mut notes = vec!["runtime=lightweight".to_string()];
    if safety_snapshot.is_some() {
        notes.push("safety_snapshot=created".to_string());
    }
    if output.access_password.is_some() {
        notes.push("access_password=rotated".to_string());
    }

    Ok(LightweightRestoreReport {
        target: target.name.clone(),
        target_ip: target.vps.ip.clone(),
        site: site_name.to_string(),
        backup_id: backup_id.to_string(),
        status: "restored".to_string(),
        fqdn: output.fqdn,
        access_user: output.access_user,
        access_password: output.access_password,
        notes,
    })
}

fn build_backup_id(label: Option<&str>) -> String {
    let base = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    match label {
        Some(value) if !value.trim().is_empty() => {
            format!("{}-{}", base, sanitize_path_name(value))
        }
        _ => base,
    }
}

fn sanitize_path_name(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' => character,
            _ => '_',
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn retention_keep_for(tier: &BackupTier) -> Option<usize> {
    match tier {
        BackupTier::Daily => Some(3),
        BackupTier::Weekly => Some(2),
        BackupTier::Manual => None,
    }
}

fn build_local_artifact(
    kind: &str,
    logical_name: &str,
    file_path: &Path,
    original_path: Option<String>,
) -> std::result::Result<BackupArtifact, CoolifyError> {
    let bytes = fs::read(file_path)?;
    Ok(BackupArtifact {
        kind: kind.to_string(),
        logical_name: logical_name.to_string(),
        relative_path: file_path
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .ok_or_else(|| {
                CoolifyError::Validation(format!(
                    "Artifacto lightweight sin file_name: {}",
                    file_path.display()
                ))
            })?,
        original_path,
        size_bytes: bytes.len() as u64,
        sha256: hash_bytes(&bytes),
    })
}

fn write_local_manifest(
    directory: &Path,
    manifest: &BackupManifest,
) -> std::result::Result<(), CoolifyError> {
    let json = serde_json::to_string_pretty(manifest)
        .map_err(|error| CoolifyError::Validation(error.to_string()))?;
    fs::write(directory.join("manifest.json"), json)?;
    Ok(())
}

fn read_local_manifest(path: &Path) -> std::result::Result<BackupManifest, CoolifyError> {
    let content = fs::read_to_string(path)?;
    serde_json::from_str(&content)
        .map_err(|error| CoolifyError::Validation(format!("Manifiesto inválido: {error}")))
}

fn validate_local_manifest(
    directory: &Path,
    manifest: &BackupManifest,
) -> std::result::Result<(), CoolifyError> {
    if manifest.artifacts.is_empty() {
        return Err(CoolifyError::Validation(
            "Backup lightweight sin artifacts".to_string(),
        ));
    }

    for artifact in &manifest.artifacts {
        /* [119A-5] canonicalize: relative_path validado antes de leer. */
        let artifact_path =
            validation::unir_relativo_seguro(directory, &artifact.relative_path, "backup")?;
        let bytes = fs::read(&artifact_path)?;
        if hash_bytes(&bytes) != artifact.sha256 {
            return Err(CoolifyError::Validation(format!(
                "Checksum inválido en {}",
                artifact.relative_path
            )));
        }
    }

    Ok(())
}

fn create_local_archive(
    source_dir: &Path,
    archive_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    let folder_name = source_dir
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            CoolifyError::Validation(format!(
                "Ruta de backup lightweight inválida: {}",
                source_dir.display()
            ))
        })?;
    let archive_file = File::create(archive_path)?;
    let encoder = GzEncoder::new(archive_file, Compression::default());
    let mut builder = Builder::new(encoder);
    builder.append_dir_all(folder_name, source_dir)?;
    builder.finish()?;
    Ok(())
}

fn cleanup_dir(path: &Path) -> std::result::Result<(), std::io::Error> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
