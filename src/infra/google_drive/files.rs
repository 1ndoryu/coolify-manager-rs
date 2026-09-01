use crate::error::{ApiError, CoolifyError};

use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::path::Path;

use super::GoogleDriveClient;

const DRIVE_FOLDER_MIME: &str = "application/vnd.google-apps.folder";
const DRIVE_FILES_URL: &str = "https://www.googleapis.com/drive/v3/files";
const DRIVE_UPLOAD_URL: &str = "https://www.googleapis.com/upload/drive/v3/files";

#[derive(Debug, Deserialize)]
struct DriveListResponse {
    files: Vec<DriveFile>,
}

#[derive(Debug, Clone, Deserialize)]
struct DriveFile {
    id: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DriveFileMetadata {
    #[allow(dead_code)]
    id: String,
    #[serde(rename = "driveId", default)]
    #[allow(dead_code)]
    drive_id: Option<String>,
    #[serde(rename = "mimeType", default)]
    mime_type: Option<String>,
    #[serde(default)]
    capabilities: Option<DriveCapabilities>,
}

#[derive(Debug, Deserialize)]
struct DriveCapabilities {
    #[serde(rename = "canAddChildren", default)]
    can_add_children: bool,
}

#[derive(Debug, Deserialize)]
struct DriveUploadResponse {
    id: String,
}

impl GoogleDriveClient {
    pub async fn upload_backup_archive(
        &self,
        site_name: &str,
        tier_name: &str,
        backup_id: &str,
        archive_path: &Path,
    ) -> std::result::Result<String, CoolifyError> {
        self.ensure_root_folder_uploadable().await?;
        let site_folder = self.ensure_folder(&self.root_folder_id, site_name).await?;
        let tier_folder = self.ensure_folder(&site_folder, tier_name).await?;
        let file_name = format!("{backup_id}.tar.gz");
        let bytes = fs::read(archive_path)?;
        let existing = self.find_file(&tier_folder, &file_name, None).await?;
        let metadata = json!({
            "name": file_name,
            "parents": [tier_folder],
        });

        self.upload_file(
            existing.as_ref().map(|file| file.id.as_str()),
            &metadata,
            bytes,
        )
        .await
    }

    pub async fn download_backup_archive(
        &self,
        site_name: &str,
        tier_name: &str,
        backup_id: &str,
        destination: &Path,
    ) -> std::result::Result<bool, CoolifyError> {
        self.ensure_root_folder_access().await?;
        let Some(site_folder) = self
            .find_file(&self.root_folder_id, site_name, Some(DRIVE_FOLDER_MIME))
            .await?
        else {
            return Ok(false);
        };
        let Some(tier_folder) = self
            .find_file(&site_folder.id, tier_name, Some(DRIVE_FOLDER_MIME))
            .await?
        else {
            return Ok(false);
        };
        let file_name = format!("{backup_id}.tar.gz");
        let Some(file) = self.find_file(&tier_folder.id, &file_name, None).await? else {
            return Ok(false);
        };

        let token = self.access_token().await?;
        let response = self
            .client
            .get(format!("{DRIVE_FILES_URL}/{}", file.id))
            .bearer_auth(token)
            .query(&[("alt", "media"), ("supportsAllDrives", "true")])
            .send()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;

        if !status.is_success() {
            return Err(ApiError::HttpError {
                status: status.as_u16(),
                body: String::from_utf8_lossy(&bytes).to_string(),
            }
            .into());
        }

        fs::write(destination, bytes)?;
        Ok(true)
    }

    pub async fn ensure_root_folder_uploadable(&self) -> std::result::Result<(), CoolifyError> {
        let metadata = self.root_folder_metadata().await?;

        if metadata.mime_type.as_deref() != Some(DRIVE_FOLDER_MIME) {
            return Err(CoolifyError::Validation(format!(
                "La ruta Google Drive '{}' no apunta a una carpeta valida",
                self.root_folder_id
            )));
        }

        /* [014A-20] Service Accounts pueden subir a carpetas de My Drive
         * siempre que el propietario las comparta como Editor con la SA.
         * El almacenamiento cuenta contra la cuota del propietario.
         * Ya no se requiere Shared Drive — solo permisos de escritura. */

        if !metadata
            .capabilities
            .as_ref()
            .map(|value| value.can_add_children)
            .unwrap_or(false)
        {
            let identity = self.auth_identity();
            return Err(CoolifyError::Validation(format!(
                "Sin permisos de escritura sobre la carpeta Google Drive '{}'. Comparte la carpeta con {identity} como Editor",
                self.root_folder_id
            )));
        }

        Ok(())
    }

    /// Lista todos los archivos (no carpetas) en una carpeta del tier de un sitio.
    /// Retorna pares (file_id, name) ordenados por nombre (contiene timestamp).
    pub async fn list_tier_files(
        &self,
        site_name: &str,
        tier_name: &str,
    ) -> std::result::Result<Vec<(String, String)>, CoolifyError> {
        self.ensure_root_folder_access().await?;
        let Some(site_folder) = self
            .find_file(&self.root_folder_id, site_name, Some(DRIVE_FOLDER_MIME))
            .await?
        else {
            return Ok(Vec::new());
        };
        let Some(tier_folder) = self
            .find_file(&site_folder.id, tier_name, Some(DRIVE_FOLDER_MIME))
            .await?
        else {
            return Ok(Vec::new());
        };

        let token = self.access_token().await?;
        let query = format!(
            "'{}' in parents and trashed = false and mimeType != '{}'",
            super::escape_query_literal(&tier_folder.id),
            DRIVE_FOLDER_MIME,
        );

        let response = self
            .client
            .get(DRIVE_FILES_URL)
            .bearer_auth(token)
            .query(&[
                ("q", query.as_str()),
                ("fields", "files(id,name)"),
                ("orderBy", "name desc"),
                ("pageSize", "1000"),
                ("supportsAllDrives", "true"),
                ("includeItemsFromAllDrives", "true"),
            ])
            .send()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;

        if !status.is_success() {
            return Err(ApiError::HttpError {
                status: status.as_u16(),
                body,
            }
            .into());
        }

        let files: DriveListResponse = serde_json::from_str(&body)
            .map_err(|error| ApiError::InvalidResponse(error.to_string()))?;
        Ok(files
            .files
            .into_iter()
            .map(|file| (file.id, file.name.unwrap_or_default()))
            .collect())
    }

    /// Elimina un archivo de Google Drive por su file_id.
    pub async fn delete_file(&self, file_id: &str) -> std::result::Result<(), CoolifyError> {
        let token = self.access_token().await?;
        let response = self
            .client
            .delete(format!("{DRIVE_FILES_URL}/{file_id}"))
            .bearer_auth(token)
            .query(&[("supportsAllDrives", "true")])
            .send()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        let status = response.status();

        if status.as_u16() == 204 || status.is_success() {
            return Ok(());
        }

        let body = response
            .text()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        Err(ApiError::HttpError {
            status: status.as_u16(),
            body,
        }
        .into())
    }

    async fn ensure_folder(
        &self,
        parent_id: &str,
        name: &str,
    ) -> std::result::Result<String, CoolifyError> {
        if let Some(folder) = self
            .find_file(parent_id, name, Some(DRIVE_FOLDER_MIME))
            .await?
        {
            return Ok(folder.id);
        }

        let metadata = json!({
            "name": name,
            "mimeType": DRIVE_FOLDER_MIME,
            "parents": [parent_id],
        });

        self.upload_file(None, &metadata, Vec::new()).await
    }

    pub async fn ensure_root_folder_access(&self) -> std::result::Result<(), CoolifyError> {
        self.root_folder_metadata().await.map(|_| ())
    }

    async fn root_folder_metadata(&self) -> std::result::Result<DriveFileMetadata, CoolifyError> {
        let token = self.access_token().await?;
        let response = self
            .client
            .get(format!("{DRIVE_FILES_URL}/{}", self.root_folder_id))
            .bearer_auth(token)
            .query(&[
                ("fields", "id,driveId,mimeType,capabilities(canAddChildren)"),
                ("supportsAllDrives", "true"),
            ])
            .send()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;

        if status.as_u16() == 404 {
            let identity = self.auth_identity();
            return Err(CoolifyError::Validation(format!(
                "La carpeta Google Drive '{}' no existe o no esta compartida con {identity}",
                self.root_folder_id
            )));
        }

        if !status.is_success() {
            return Err(ApiError::HttpError {
                status: status.as_u16(),
                body,
            }
            .into());
        }

        serde_json::from_str(&body)
            .map_err(|error| CoolifyError::from(ApiError::InvalidResponse(error.to_string())))
    }

    async fn upload_file(
        &self,
        file_id: Option<&str>,
        metadata: &serde_json::Value,
        bytes: Vec<u8>,
    ) -> std::result::Result<String, CoolifyError> {
        let token = self.access_token().await?;
        let metadata_part = Part::text(metadata.to_string())
            .mime_str("application/json; charset=UTF-8")
            .map_err(|error| {
                CoolifyError::Validation(format!("Metadata multipart invalido: {error}"))
            })?;
        let media_part = Part::bytes(bytes)
            .mime_str("application/gzip")
            .map_err(|error| {
                CoolifyError::Validation(format!("Media multipart invalido: {error}"))
            })?;
        let form = Form::new()
            .part("metadata", metadata_part)
            .part("media", media_part);

        let request = match file_id {
            Some(file_id) => self
                .client
                .patch(format!("{DRIVE_UPLOAD_URL}/{file_id}"))
                .query(&[("uploadType", "multipart"), ("supportsAllDrives", "true")]),
            None => self
                .client
                .post(DRIVE_UPLOAD_URL)
                .query(&[("uploadType", "multipart"), ("supportsAllDrives", "true")]),
        };

        let response = request
            .bearer_auth(token)
            .multipart(form)
            .send()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;

        if !status.is_success() {
            if status.as_u16() == 403 && body.contains("storageQuotaExceeded") {
                return Err(CoolifyError::Validation(
                    "Google Drive rechazo la subida por quota. Si usas service account, cambia a OAuth con 'auth-drive'. Si usas OAuth, verifica tu almacenamiento en drive.google.com".to_string(),
                ));
            }
            return Err(ApiError::HttpError {
                status: status.as_u16(),
                body,
            }
            .into());
        }

        let uploaded: DriveUploadResponse = serde_json::from_str(&body)
            .map_err(|error| ApiError::InvalidResponse(error.to_string()))?;
        Ok(uploaded.id)
    }

    async fn find_file(
        &self,
        parent_id: &str,
        name: &str,
        mime_type: Option<&str>,
    ) -> std::result::Result<Option<DriveFile>, CoolifyError> {
        let token = self.access_token().await?;
        let mut query = format!(
            "name = '{}' and '{}' in parents and trashed = false",
            super::escape_query_literal(name),
            super::escape_query_literal(parent_id)
        );
        if let Some(mime_type) = mime_type {
            query.push_str(&format!(
                " and mimeType = '{}'",
                super::escape_query_literal(mime_type)
            ));
        }

        let response = self
            .client
            .get(DRIVE_FILES_URL)
            .bearer_auth(token)
            .query(&[
                ("q", query.as_str()),
                ("fields", "files(id)"),
                ("supportsAllDrives", "true"),
                ("includeItemsFromAllDrives", "true"),
            ])
            .send()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;

        if !status.is_success() {
            return Err(ApiError::HttpError {
                status: status.as_u16(),
                body,
            }
            .into());
        }

        let files: DriveListResponse = serde_json::from_str(&body)
            .map_err(|error| ApiError::InvalidResponse(error.to_string()))?;
        Ok(files.files.into_iter().next())
    }
}