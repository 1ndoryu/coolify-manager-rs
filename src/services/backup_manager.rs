use crate::config::{RemoteBackupConfig, Settings};
use crate::domain::{BackupTier, DatabaseEngine, SiteConfig};
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::google_drive::GoogleDriveClient;
use crate::infra::ssh_backup::SshBackupClient;
use crate::infra::ssh_client::SshClient;
use crate::services::{database_manager, health_manager, site_capabilities};

use chrono::{DateTime, Local, Utc};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use tar::{Archive, Builder};

const MANIFEST_FILE: &str = "manifest.json";

/* [N1] Abstraccion multi-backend para almacenamiento remoto de backups.
 * Soporta Google Drive (legacy) y SSH a VPS remoto (recomendado).
 * Cada variante implementa las mismas operaciones: validar, subir, bajar, listar, eliminar. */
enum RemoteClient {
    GoogleDrive(GoogleDriveClient),
    SshRemote(SshBackupClient),
}

impl RemoteClient {
    async fn ensure_writable(&self) -> std::result::Result<(), CoolifyError> {
        match self {
            Self::GoogleDrive(client) => client.ensure_root_folder_uploadable().await,
            Self::SshRemote(client) => client.ensure_writable().await,
        }
    }

    async fn upload(
        &self,
        site_name: &str,
        tier: &str,
        backup_id: &str,
        local_path: &std::path::Path,
    ) -> std::result::Result<String, CoolifyError> {
        match self {
            Self::GoogleDrive(client) => {
                client
                    .upload_backup_archive(site_name, tier, backup_id, local_path)
                    .await
            }
            Self::SshRemote(client) => {
                client
                    .upload_backup_archive(site_name, tier, backup_id, local_path)
                    .await
            }
        }
    }

    async fn download(
        &self,
        site_name: &str,
        tier: &str,
        backup_id: &str,
        local_path: &std::path::Path,
    ) -> std::result::Result<bool, CoolifyError> {
        match self {
            Self::GoogleDrive(client) => {
                client
                    .download_backup_archive(site_name, tier, backup_id, local_path)
                    .await
            }
            Self::SshRemote(client) => {
                client
                    .download_backup_archive(site_name, tier, backup_id, local_path)
                    .await
            }
        }
    }

    async fn list_tier_files(
        &self,
        site_name: &str,
        tier: &str,
    ) -> std::result::Result<Vec<(String, String)>, CoolifyError> {
        match self {
            Self::GoogleDrive(client) => client.list_tier_files(site_name, tier).await,
            Self::SshRemote(client) => client.list_tier_files(site_name, tier).await,
        }
    }

    async fn delete_file(&self, id_or_path: &str) -> std::result::Result<(), CoolifyError> {
        match self {
            Self::GoogleDrive(client) => client.delete_file(id_or_path).await,
            Self::SshRemote(client) => client.delete_file(id_or_path).await,
        }
    }

    fn backend_name(&self) -> &str {
        match self {
            Self::GoogleDrive(_) => "Google Drive",
            Self::SshRemote(_) => "SSH VPS",
        }
    }

    fn supports_direct_transfer(&self) -> bool {
        match self {
            Self::SshRemote(client) => client.supports_direct_transfer(),
            _ => false,
        }
    }

    fn as_ssh_remote(&self) -> Option<&SshBackupClient> {
        match self {
            Self::SshRemote(client) => Some(client),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupRemoteMode {
    FollowSettings,
    Skip,
}

#[derive(Debug, Clone)]
pub struct BackupExecutionOptions {
    pub source_paths_override: Option<Vec<String>>,
    pub remote_mode: BackupRemoteMode,
}

impl Default for BackupExecutionOptions {
    fn default() -> Self {
        Self {
            source_paths_override: None,
            remote_mode: BackupRemoteMode::FollowSettings,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BackupStatus {
    Creating,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupArtifact {
    pub kind: String,
    pub logical_name: String,
    pub relative_path: String,
    pub original_path: Option<String>,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub backup_id: String,
    pub site_name: String,
    pub tier: BackupTier,
    pub status: BackupStatus,
    pub created_at: DateTime<Utc>,
    pub label: Option<String>,
    pub artifacts: Vec<BackupArtifact>,
    pub notes: Vec<String>,
}

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
    let staging_dir = backup_root.join(format!(".staging-{backup_id}"));
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

    let caps = site_capabilities::resolve(site);
    let source_paths = caps.persistent_paths.clone();
    let stack_uuid = site.stack_uuid.as_deref().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{}' sin stackUuid", site.nombre))
    })?;
    let app_container = caps.resolve_app_container(ssh, stack_uuid).await?;

    let backup_result = async {
        for binding in &caps.database_bindings {
            let db_container = caps
                .resolve_database_container(ssh, stack_uuid, binding)
                .await?;
            let output_file = staging_dir.join(format!("db-{}.sql", binding.logical_name));
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
            let archive_path = staging_dir.join(format!("files-{}.tar.gz", safe_name));
            archive_container_path(ssh, &app_container, source_path, &archive_path).await?;
            manifest.artifacts.push(build_artifact(
                "files",
                &safe_name,
                &archive_path,
                Some(source_path.clone()),
            )?);
        }

        validate_backup_dir(&staging_dir, &manifest)?;
        Ok::<(), CoolifyError>(())
    }
    .await;

    if let Err(error) = backup_result {
        manifest.status = BackupStatus::Failed;
        manifest.notes.push(error.to_string());
        let _ = cleanup_dir(&staging_dir);
        return Err(error);
    }

    /* Crear archive tar.gz empaquetando todo el staging */
    let archive_path = backup_root.join(format!("{backup_id}.tar.gz"));
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
            manifest
                .notes
                .push(format!("remote.{}.id={file_id}", remote_client.backend_name()));
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

pub async fn list_site_backups(
    settings: &Settings,
    config_path: &Path,
    site_name: &str,
) -> std::result::Result<Vec<DriveBackupEntry>, CoolifyError> {
    let remote_client = build_remote_client(settings, config_path).await?;
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

/// Entrada de backup en Google Drive para listados.
#[derive(Debug, Clone)]
pub struct DriveBackupEntry {
    pub backup_id: String,
    pub tier: BackupTier,
    pub file_id: String,
    pub file_name: String,
}

pub async fn restore_site_backup(
    settings: &Settings,
    config_path: &Path,
    site: &SiteConfig,
    ssh: &SshClient,
    backup_id: &str,
    skip_safety_snapshot: bool,
) -> std::result::Result<(), CoolifyError> {
    /* Siempre descargar desde almacenamiento remoto */
    let manifest_dir = materialize_remote_backup(settings, config_path, &site.nombre, backup_id)
        .await?
        .ok_or_else(|| {
            CoolifyError::Validation(format!(
                "Backup '{}' no encontrado en almacenamiento remoto para '{}'",
                backup_id, site.nombre
            ))
        })?;
    let manifest = read_manifest(&manifest_dir.join(MANIFEST_FILE))?;
    validate_backup_dir(&manifest_dir, &manifest)?;

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

    let restore_result = async {
        let caps = site_capabilities::resolve(site);
        let stack_uuid = site.stack_uuid.as_deref().ok_or_else(|| {
            CoolifyError::Validation(format!("Sitio '{}' sin stackUuid", site.nombre))
        })?;
        let app_container = caps.resolve_app_container(ssh, stack_uuid).await?;

        for artifact in &manifest.artifacts {
            let local_path = manifest_dir.join(&artifact.relative_path);
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
                    restore_archive_to_container(ssh, &app_container, &local_path, target_path)
                        .await?;
                }
                other => {
                    return Err(CoolifyError::Validation(format!(
                        "Tipo de artifacto desconocido: {other}"
                    )));
                }
            }
        }

        health_manager::assert_site_healthy(settings, site, ssh).await?;
        Ok::<(), CoolifyError>(())
    }
    .await;

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

fn resolve_backup_root(settings: &Settings, config_path: &Path) -> PathBuf {
    let relative = PathBuf::from(&settings.backup_storage.local_dir);
    if relative.is_absolute() {
        return relative;
    }
    config_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(relative)
}

fn build_backup_id(label: Option<&str>) -> String {
    let base = Local::now().format("%Y%m%d_%H%M%S").to_string();
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

#[allow(dead_code)]
fn write_manifest(
    directory: &Path,
    manifest: &BackupManifest,
) -> std::result::Result<(), CoolifyError> {
    let json = serde_json::to_string_pretty(manifest).map_err(|error| {
        CoolifyError::Validation(format!("No se pudo serializar manifiesto: {error}"))
    })?;
    fs::write(directory.join(MANIFEST_FILE), json)?;
    Ok(())
}

fn read_manifest(path: &Path) -> std::result::Result<BackupManifest, CoolifyError> {
    let content = fs::read_to_string(path)?;
    serde_json::from_str(&content).map_err(|error| {
        CoolifyError::Validation(format!("Manifiesto invalido '{}': {error}", path.display()))
    })
}

fn build_artifact(
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

fn validate_backup_dir(
    directory: &Path,
    manifest: &BackupManifest,
) -> std::result::Result<(), CoolifyError> {
    if manifest.artifacts.is_empty() {
        return Err(CoolifyError::Validation("Backup sin artifacts".to_string()));
    }

    for artifact in &manifest.artifacts {
        let artifact_path = directory.join(&artifact.relative_path);
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
async fn build_remote_client(
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
async fn create_site_backup_server_side(
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

    /* Crear staging dir en VPS1 */
    let result = ssh
        .execute(&format!("mkdir -p '{staging_dir}'"))
        .await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo crear staging en VPS1: {}",
            result.stderr
        )));
    }

    let caps = site_capabilities::resolve(site);
    let source_paths = caps.persistent_paths.clone();
    let stack_uuid = site.stack_uuid.as_deref().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{}' sin stackUuid", site.nombre))
    })?;
    let app_container = caps.resolve_app_container(ssh, stack_uuid).await?;

    let mut artifacts = Vec::new();

    let backup_result = async {
        /* Fase 1: Exportar bases de datos al staging EN VPS1 */
        for binding in &caps.database_bindings {
            let db_container = caps
                .resolve_database_container(ssh, stack_uuid, binding)
                .await?;
            let host_output = format!("{}/db-{}.sql", staging_dir, binding.logical_name);
            export_database_binding_to_host(
                settings,
                site,
                ssh,
                &app_container,
                &db_container,
                binding.engine.clone(),
                binding.logical_name,
                &host_output,
            )
            .await?;
            artifacts.push(build_remote_artifact(
                ssh,
                "database",
                binding.logical_name,
                &host_output,
                None,
            ).await?);
        }

        /* Fase 2: Archivar filesystem al staging EN VPS1 */
        for source_path in &source_paths {
            let safe_name = sanitize_path_name(source_path);
            let host_output = format!("{}/files-{}.tar.gz", staging_dir, safe_name);
            archive_container_path_to_host(ssh, &app_container, source_path, &host_output).await?;
            artifacts.push(build_remote_artifact(
                ssh,
                "files",
                &safe_name,
                &host_output,
                Some(source_path.clone()),
            ).await?);
        }

        Ok::<(), CoolifyError>(())
    }
    .await;

    if let Err(error) = backup_result {
        let _ = ssh.execute(&format!("rm -rf '{staging_dir}'")).await;
        return Err(error);
    }

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

    /* Escribir manifest via cat heredoc (el JSON solo usa comillas dobles, seguro con delimitador single-quoted) */
    let write_cmd = format!(
        "cat > '{staging_dir}/manifest.json' << 'CM_MANIFEST_EOF'\n{manifest_json}\nCM_MANIFEST_EOF"
    );
    let result = ssh.execute(&write_cmd).await?;
    if !result.success() {
        let _ = ssh.execute(&format!("rm -rf '{staging_dir}'")).await;
        return Err(CoolifyError::Validation(format!(
            "No se pudo escribir manifest en VPS1: {}",
            result.stderr
        )));
    }

    /* Fase 4: Crear tar.gz en VPS1 */
    let staging_name = format!(".staging-{backup_id}");
    let archive_path = format!("/tmp/cm-backup-{backup_id}.tar.gz");

    /* Renombrar staging para que la estructura interna del tar sea correcta */
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

    /* Eliminar staging ya empaquetado */
    let _ = ssh.execute(&format!("rm -rf '{renamed}'")).await;

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
            manifest
                .notes
                .push(format!("remote.{}.id={file_id}", remote_client.backend_name()));
            manifest
                .notes
                .push("transfer.mode=direct-vps1-to-vps2".to_string());
            println!(
                "Backup '{}' transferido VPS1→VPS2 (id: {file_id})",
                backup_id,
            );

            if let Err(prune_error) = prune_retention(remote_client, site, &tier).await {
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

/* Exporta base de datos dejando el SQL en VPS1 (server-side). */
#[allow(clippy::too_many_arguments)]
async fn export_database_binding_to_host(
    _settings: &Settings,
    _site: &SiteConfig,
    ssh: &SshClient,
    app_container: &str,
    db_container: &str,
    engine: DatabaseEngine,
    _logical_name: &str,
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
    let container_archive = format!("/tmp/cm_backup_{}.tar.gz", sanitize_path_name(source_path));
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
            stderr: format!("No se pudo empaquetar '{source_path}' (server-side): {}", result.stderr),
        });
    }

    /* docker cp del contenedor al host VPS1 (I/O local, instantaneo) */
    docker::copy_from_container_to_host(ssh, container_id, &container_archive, host_output).await?;
    let _ = docker::docker_exec(ssh, container_id, &format!("rm -f {container_archive}")).await;
    Ok(())
}

/* Construye un BackupArtifact computando hash y tamaño en VPS1 (sin descargar a local). */
async fn build_remote_artifact(
    ssh: &SshClient,
    kind: &str,
    logical_name: &str,
    host_path: &str,
    original_path: Option<String>,
) -> std::result::Result<BackupArtifact, CoolifyError> {
    let hash_result = ssh
        .execute(&format!("sha256sum '{}' | awk '{{print $1}}'", host_path))
        .await?;
    let size_result = ssh
        .execute(&format!("stat -c%s '{}'", host_path))
        .await?;

    let sha256 = hash_result.stdout.trim().to_string();
    let size_bytes: u64 = size_result
        .stdout
        .trim()
        .parse()
        .map_err(|_| {
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

fn cleanup_dir(path: &Path) -> std::result::Result<(), std::io::Error> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

async fn materialize_remote_backup(
    settings: &Settings,
    config_path: &Path,
    site_name: &str,
    backup_id: &str,
) -> std::result::Result<Option<PathBuf>, CoolifyError> {
    let client = build_remote_client(settings, config_path).await?;
    let backup_root = resolve_backup_root(settings, config_path);
    let temp_root = backup_root.join(format!(".restore-{backup_id}"));
    fs::create_dir_all(&temp_root)?;

    for tier in [BackupTier::Daily, BackupTier::Weekly, BackupTier::Manual] {
        let tier_name = tier.to_string();
        let archive_path = temp_root.join(format!("{backup_id}.tar.gz"));

        if !client
            .download(site_name, &tier_name, backup_id, &archive_path)
            .await?
        {
            continue;
        }

        extract_backup_archive(&archive_path, &temp_root)?;
        let _ = fs::remove_file(&archive_path);

        let candidate = temp_root.join(backup_id);
        if candidate.exists() {
            return Ok(Some(candidate));
        }
    }

    let _ = cleanup_dir(&temp_root);
    Ok(None)
}

fn create_backup_archive(
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

fn extract_backup_archive(
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
async fn prune_retention(
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
async fn export_database_binding(
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
async fn restore_database_artifact(
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

async fn archive_container_path(
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

async fn restore_archive_to_container(
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
