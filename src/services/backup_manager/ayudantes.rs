/* Split 119A-5 de backup_manager.rs — ayudantes locales/remotos y materializacion.
 * Codigo verbatim del original (lineas 642-793 + 1103-1468); solo cambia visibilidad. */

use super::tipos::{BackupArtifact, BackupManifest, RemoteClient, MANIFEST_FILE};
use crate::config::{RemoteBackupConfig, Settings};
use crate::domain::{BackupTier, DatabaseEngine, SiteConfig};
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::google_drive::GoogleDriveClient;
use crate::infra::ssh_backup::SshBackupClient;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::{database_manager, site_capabilities};

use chrono::Local;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use tar::{Archive, Builder};

pub(super) fn resolve_backup_root(settings: &Settings, config_path: &Path) -> PathBuf {
    let relative = PathBuf::from(&settings.backup_storage.local_dir);
    if relative.is_absolute() {
        return relative;
    }
    /* [119A-5] canonicalize: local_dir de config; se rechaza traversal `..`. */
    if validation::validar_ruta_relativa(&settings.backup_storage.local_dir, "backup").is_err() {
        return config_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("backups");
    }
    let base = config_path.parent().unwrap_or(Path::new("."));
    /* [119A-5] canonicalize: unir_relativo_seguro valida + canonicalize + starts_with. */
    let joined =
        validation::unir_relativo_seguro(base, &settings.backup_storage.local_dir, "backup")
            .unwrap_or_else(|_| base.join("backups"));
    /* canonicalize cuando existe para normalizar el root. */
    if let (Ok(canon_base), Ok(canon_joined)) = (base.canonicalize(), joined.canonicalize()) {
        if canon_joined.starts_with(&canon_base) {
            return canon_joined;
        }
    }
    joined
}

pub(super) fn build_backup_id(label: Option<&str>) -> String {
    let base = Local::now().format("%Y%m%d_%H%M%S").to_string();
    match label {
        Some(value) if !value.trim().is_empty() => {
            format!("{}-{}", base, sanitize_path_name(value))
        }
        _ => base,
    }
}

pub(super) fn sanitize_path_name(value: &str) -> String {
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

#[allow(dead_code)]
pub(super) fn write_manifest(
    directory: &Path,
    manifest: &BackupManifest,
) -> std::result::Result<(), CoolifyError> {
    let json = serde_json::to_string_pretty(manifest).map_err(|error| {
        CoolifyError::Validation(format!("No se pudo serializar manifiesto: {error}"))
    })?;
    fs::write(directory.join(MANIFEST_FILE), json)?;
    Ok(())
}

pub(super) fn read_manifest(path: &Path) -> std::result::Result<BackupManifest, CoolifyError> {
    let content = fs::read_to_string(path)?;
    serde_json::from_str(&content).map_err(|error| {
        CoolifyError::Validation(format!("Manifiesto invalido '{}': {error}", path.display()))
    })
}

pub(super) fn build_artifact(
    kind: &str,
    logical_name: &str,
    file_path: &Path,
    original_path: Option<String>,
) -> std::result::Result<BackupArtifact, CoolifyError> {
    let bytes = fs::read(file_path)?;
    let size_bytes = bytes.len() as u64;
    let sha256 = hash_bytes(&bytes);
    Ok(BackupArtifact {
        kind: kind.to_string(),
        logical_name: logical_name.to_string(),
        relative_path: file_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .ok_or_else(|| {
                CoolifyError::Validation(format!("Ruta invalida: {}", file_path.display()))
            })?,
        original_path,
        size_bytes,
        sha256,
    })
}

pub(super) fn validate_backup_dir(
    directory: &Path,
    manifest: &BackupManifest,
) -> std::result::Result<(), CoolifyError> {
    if manifest.artifacts.is_empty() {
        return Err(CoolifyError::Validation("Backup sin artifacts".to_string()));
    }

    for artifact in &manifest.artifacts {
        /* [119A-5] canonicalize: relative_path validado antes de leer. */
        let artifact_path =
            validation::unir_relativo_seguro(directory, &artifact.relative_path, "backup")?;
        if !artifact_path.exists() {
            return Err(CoolifyError::Validation(format!(
                "Artifacto faltante: {}",
                artifact.relative_path
            )));
        }
        let bytes = fs::read(&artifact_path)?;
        if bytes.is_empty() {
            return Err(CoolifyError::Validation(format!(
                "Artifacto vacio: {}",
                artifact.relative_path
            )));
        }
        let actual_hash = hash_bytes(&bytes);
        if actual_hash != artifact.sha256 {
            return Err(CoolifyError::Validation(format!(
                "Checksum invalido en {}",
                artifact.relative_path
            )));
        }
    }

    Ok(())
}

/* [N1] Construye el cliente remoto segun la configuracion de backupStorage.
 * Soporta Google Drive (legacy) y SSH VPS (recomendado). */
pub(super) async fn build_remote_client(
    settings: &Settings,
    config_path: &Path,
) -> std::result::Result<RemoteClient, CoolifyError> {
    let remote = settings.backup_storage.remote.as_ref().ok_or_else(|| {
        CoolifyError::Validation(
            "No hay configuracion remota de backup en settings.json (backupStorage.remote)"
                .to_string(),
        )
    })?;
    match remote {
        RemoteBackupConfig::GoogleDrive(config) => {
            let client = GoogleDriveClient::new(config_path, config)?;
            Ok(RemoteClient::GoogleDrive(client))
        }
        RemoteBackupConfig::SshRemote(config) => {
            let client = SshBackupClient::new(config).await?;
            Ok(RemoteClient::SshRemote(client))
        }
    }
}

/* Construye un BackupArtifact computando hash y tamaño en VPS1 (sin descargar a local). */
pub(super) async fn build_remote_artifact(
    ssh: &SshClient,
    kind: &str,
    logical_name: &str,
    host_path: &str,
    original_path: Option<String>,
) -> std::result::Result<BackupArtifact, CoolifyError> {
    let hash_result = ssh
        .execute(&format!("sha256sum '{}' | awk '{{print $1}}'", host_path))
        .await?;
    let size_result = ssh.execute(&format!("stat -c%s '{}'", host_path)).await?;

    let sha256 = hash_result.stdout.trim().to_string();
    let size_bytes: u64 = size_result.stdout.trim().parse().map_err(|_| {
        CoolifyError::Validation(format!(
            "No se pudo obtener tamano de VPS1:{}: {}",
            host_path,
            size_result.stdout.trim()
        ))
    })?;

    if sha256.is_empty() || size_bytes == 0 {
        return Err(CoolifyError::Validation(format!(
            "Artifact vacio o sin hash en VPS1:{}",
            host_path,
        )));
    }

    let file_name = host_path
        .rsplit('/')
        .next()
        .unwrap_or(host_path)
        .to_string();

    tracing::info!(
        "Artifact server-side: {} ({:.1} MB, sha256={}...)",
        file_name,
        size_bytes as f64 / 1_048_576.0,
        &sha256[..sha256.len().min(12)]
    );

    Ok(BackupArtifact {
        kind: kind.to_string(),
        logical_name: logical_name.to_string(),
        relative_path: file_name,
        original_path,
        size_bytes,
        sha256,
    })
}

pub(super) fn cleanup_dir(path: &Path) -> std::result::Result<(), std::io::Error> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

pub(super) async fn materialize_remote_backup(
    settings: &Settings,
    config_path: &Path,
    site_name: &str,
    backup_id: &str,
) -> std::result::Result<Option<PathBuf>, CoolifyError> {
    let client = build_remote_client(settings, config_path).await?;
    let backup_root = resolve_backup_root(settings, config_path);
    /* [119A-5] canonicalize: backup_id viene de CLI, se valida como segmento. */
    validation::validar_segmento_ruta(backup_id, "backup")?;
    let restore_name = format!(".restore-{backup_id}");
    validation::validar_segmento_ruta(&restore_name, "backup")?;
    let temp_root = validation::join_segmento_seguro(&backup_root, &restore_name, "backup")?;
    fs::create_dir_all(&temp_root)?;

    for tier in [BackupTier::Daily, BackupTier::Weekly, BackupTier::Manual] {
        let tier_name = tier.to_string();
        let archive_name = format!("{backup_id}.tar.gz");
        validation::validar_segmento_ruta(&archive_name, "backup")?;
        let archive_path = validation::join_segmento_seguro(&temp_root, &archive_name, "backup")?;

        if !client
            .download(site_name, &tier_name, backup_id, &archive_path)
            .await?
        {
            continue;
        }

        extract_backup_archive(&archive_path, &temp_root)?;
        let _ = fs::remove_file(&archive_path);

        let candidate = validation::join_segmento_seguro(&temp_root, backup_id, "backup")?;
        if candidate.exists() {
            return Ok(Some(candidate));
        }
    }

    let _ = cleanup_dir(&temp_root);
    Ok(None)
}

pub(super) fn create_backup_archive(
    source_dir: &Path,
    archive_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    let backup_name = source_dir
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            CoolifyError::Validation(format!("Ruta de backup invalida: {}", source_dir.display()))
        })?;
    let archive_file = File::create(archive_path)?;
    let encoder = GzEncoder::new(archive_file, Compression::default());
    let mut builder = Builder::new(encoder);
    builder.append_dir_all(backup_name, source_dir)?;
    builder.finish()?;
    Ok(())
}

pub(super) fn extract_backup_archive(
    archive_path: &Path,
    destination_root: &Path,
) -> std::result::Result<(), CoolifyError> {
    let archive_file = File::open(archive_path)?;
    let decoder = GzDecoder::new(archive_file);
    let mut archive = Archive::new(decoder);
    archive.unpack(destination_root)?;
    Ok(())
}

/// Poda backups antiguos en el almacenamiento remoto segun la politica de retencion del sitio.
/// Los nombres de archivo contienen timestamp (YYYYmmdd_HHMMSS), se ordenan desc y se conservan los N mas recientes.
pub(super) async fn prune_retention(
    remote_client: &RemoteClient,
    site: &SiteConfig,
    tier: &BackupTier,
) -> std::result::Result<(), CoolifyError> {
    let keep = match tier {
        BackupTier::Daily => site.backup_policy.daily_keep,
        BackupTier::Weekly => site.backup_policy.weekly_keep,
        BackupTier::Manual => return Ok(()),
    };

    let tier_name = tier.to_string();
    let files = remote_client
        .list_tier_files(&site.nombre, &tier_name)
        .await?;

    /* Los archivos ya vienen ordenados desc por nombre (timestamp). Eliminar los que sobran. */
    let to_delete: Vec<_> = files.into_iter().skip(keep).collect();
    for (file_id, name) in &to_delete {
        tracing::info!("Eliminando backup antiguo: {name} (id: {file_id})");
        remote_client.delete_file(file_id).await?;
    }

    if !to_delete.is_empty() {
        println!(
            "Retencion: eliminados {} backup(s) antiguos del tier {tier_name}",
            to_delete.len()
        );
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn export_database_binding(
    _settings: &Settings,
    _site: &SiteConfig,
    ssh: &SshClient,
    app_container: &str,
    db_container: &str,
    engine: DatabaseEngine,
    _logical_name: &str,
    output_file: &Path,
) -> std::result::Result<(), CoolifyError> {
    match engine {
        DatabaseEngine::Mariadb => {
            let (db_name, db_user, db_password) =
                database_manager::resolve_wordpress_credentials(ssh, app_container).await?;
            database_manager::export_database(
                ssh,
                db_container,
                &db_name,
                &db_user,
                &db_password,
                output_file,
            )
            .await
        }
        DatabaseEngine::Postgres => {
            let (db_name, db_user, db_password) =
                database_manager::resolve_postgres_credentials(ssh, app_container).await?;
            database_manager::export_postgres_database(
                ssh,
                db_container,
                &db_name,
                &db_user,
                &db_password,
                output_file,
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn restore_database_artifact(
    _settings: &Settings,
    _site: &SiteConfig,
    ssh: &SshClient,
    app_container: &str,
    caps: &site_capabilities::SiteCapabilities,
    stack_uuid: &str,
    artifact: &BackupArtifact,
    local_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    let binding = caps
        .database_bindings
        .iter()
        .find(|candidate| candidate.logical_name == artifact.logical_name)
        .ok_or_else(|| {
            CoolifyError::Validation(format!(
                "No existe binding DB para '{}'",
                artifact.logical_name
            ))
        })?;
    let db_container = caps
        .resolve_database_container(ssh, stack_uuid, binding)
        .await?;
    match binding.engine {
        DatabaseEngine::Mariadb => {
            let (db_name, db_user, db_password) =
                database_manager::resolve_wordpress_credentials(ssh, app_container).await?;
            database_manager::import_database(
                ssh,
                &db_container,
                local_path,
                &db_name,
                &db_user,
                &db_password,
            )
            .await
        }
        DatabaseEngine::Postgres => {
            let (db_name, db_user, db_password) =
                database_manager::resolve_postgres_credentials(ssh, app_container).await?;
            database_manager::import_postgres_database(
                ssh,
                &db_container,
                local_path,
                &db_name,
                &db_user,
                &db_password,
            )
            .await
        }
    }
}

pub(super) async fn archive_container_path(
    ssh: &SshClient,
    container_id: &str,
    source_path: &str,
    local_output: &Path,
) -> std::result::Result<(), CoolifyError> {
    let remote_archive = format!("/tmp/cm_backup_{}.tar.gz", sanitize_path_name(source_path));
    let stripped = source_path.trim_start_matches('/');
    let command = format!(
        "test -e {path} && tar --warning=no-file-changed -czf {archive} -C / {stripped}",
        path = source_path,
        archive = remote_archive,
        stripped = stripped,
    );
    let result = docker::docker_exec(ssh, container_id, &command).await?;
    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("No se pudo empaquetar '{source_path}': {}", result.stderr),
        });
    }
    docker::copy_from_container(ssh, container_id, &remote_archive, local_output).await?;
    let _ = docker::docker_exec(ssh, container_id, &format!("rm -f {remote_archive}")).await;
    Ok(())
}

pub(super) async fn restore_archive_to_container(
    ssh: &SshClient,
    container_id: &str,
    local_archive: &Path,
    target_path: &str,
) -> std::result::Result<(), CoolifyError> {
    let remote_archive = format!("/tmp/cm_restore_{}.tar.gz", sanitize_path_name(target_path));
    docker::copy_to_container(ssh, local_archive, container_id, &remote_archive).await?;
    let command = format!(
        "mkdir -p {target_parent} && tar -xzf {archive} -C / && rm -f {archive}",
        target_parent = parent_dir(target_path),
        archive = remote_archive,
    );
    let result = docker::docker_exec(ssh, container_id, &command).await?;
    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("No se pudo restaurar '{target_path}': {}", result.stderr),
        });
    }
    Ok(())
}

fn parent_dir(path: &str) -> String {
    Path::new(path)
        .parent()
        .map(|parent| parent.display().to_string())
        .unwrap_or_else(|| "/".to_string())
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut value = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut value, "{:02x}", byte);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_sanitize_path_name() {
        assert_eq!(
            sanitize_path_name("/var/www/html/wp-content"),
            "var_www_html_wp_content"
        );
        assert_eq!(sanitize_path_name("pre-restore"), "pre_restore");
    }

    #[test]
    fn test_hash_bytes_stable() {
        assert_eq!(
            hash_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_backup_archive_roundtrip() {
        let temp = tempdir().unwrap();
        let backup_dir = temp.path().join("20260315_000000-test");
        fs::create_dir_all(&backup_dir).unwrap();
        fs::write(backup_dir.join("manifest.json"), "{}").unwrap();
        fs::write(backup_dir.join("db-wordpress.sql"), "select 1;").unwrap();

        let archive_path = temp.path().join("backup.tar.gz");
        create_backup_archive(&backup_dir, &archive_path).unwrap();
        fs::remove_dir_all(&backup_dir).unwrap();
        extract_backup_archive(&archive_path, temp.path()).unwrap();

        assert!(backup_dir.join("manifest.json").exists());
        assert!(backup_dir.join("db-wordpress.sql").exists());
    }
}
