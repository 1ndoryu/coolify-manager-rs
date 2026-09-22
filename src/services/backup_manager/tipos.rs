/* Split 119A-5 de backup_manager.rs — tipos compartidos del motor de backups.
 * Re-exportados sin cambios desde backup_manager/mod.rs (ruta externa intacta). */

use crate::domain::BackupTier;
use crate::error::CoolifyError;
use crate::infra::google_drive::GoogleDriveClient;
use crate::infra::ssh_backup::SshBackupClient;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub(super) const MANIFEST_FILE: &str = "manifest.json";

/* [N1] Abstraccion multi-backend para almacenamiento remoto de backups.
 * Soporta Google Drive (legacy) y SSH a VPS remoto (recomendado).
 * Cada variante implementa las mismas operaciones: validar, subir, bajar, listar, eliminar. */
pub(super) enum RemoteClient {
    GoogleDrive(GoogleDriveClient),
    SshRemote(SshBackupClient),
}

impl RemoteClient {
    pub(super) async fn ensure_writable(&self) -> std::result::Result<(), CoolifyError> {
        match self {
            Self::GoogleDrive(client) => client.ensure_root_folder_uploadable().await,
            Self::SshRemote(client) => client.ensure_writable().await,
        }
    }

    pub(super) async fn upload(
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

    pub(super) async fn download(
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

    pub(super) async fn list_tier_files(
        &self,
        site_name: &str,
        tier: &str,
    ) -> std::result::Result<Vec<(String, String)>, CoolifyError> {
        match self {
            Self::GoogleDrive(client) => client.list_tier_files(site_name, tier).await,
            Self::SshRemote(client) => client.list_tier_files(site_name, tier).await,
        }
    }

    pub(super) async fn delete_file(
        &self,
        id_or_path: &str,
    ) -> std::result::Result<(), CoolifyError> {
        match self {
            Self::GoogleDrive(client) => client.delete_file(id_or_path).await,
            Self::SshRemote(client) => client.delete_file(id_or_path).await,
        }
    }

    pub(super) fn backend_name(&self) -> &str {
        match self {
            Self::GoogleDrive(_) => "Google Drive",
            Self::SshRemote(_) => "SSH VPS",
        }
    }

    pub(super) fn supports_direct_transfer(&self) -> bool {
        match self {
            Self::SshRemote(client) => client.supports_direct_transfer(),
            _ => false,
        }
    }

    pub(super) fn as_ssh_remote(&self) -> Option<&SshBackupClient> {
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

#[derive(Debug, Clone)]
pub struct SiteBackupEntries {
    pub site_name: String,
    pub entries: Vec<DriveBackupEntry>,
}

#[derive(Debug, Clone)]
pub struct SiteBackupListFailure {
    pub site_name: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct SiteBackupListReport {
    pub sites: Vec<SiteBackupEntries>,
    pub errors: Vec<SiteBackupListFailure>,
}

/// Entrada de backup en Google Drive para listados.
#[derive(Debug, Clone)]
pub struct DriveBackupEntry {
    pub backup_id: String,
    pub tier: BackupTier,
    pub file_id: String,
    pub file_name: String,
}
