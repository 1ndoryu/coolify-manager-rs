use crate::config::GoogleDriveBackupConfig;
use crate::error::{ApiError, CoolifyError};

use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::multipart::{Form, Part};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive";
const DRIVE_FOLDER_MIME: &str = "application/vnd.google-apps.folder";
const DRIVE_FILES_URL: &str = "https://www.googleapis.com/drive/v3/files";
const DRIVE_UPLOAD_URL: &str = "https://www.googleapis.com/upload/drive/v3/files";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

pub struct GoogleDriveClient {
    client: Client,
    auth: DriveAuthMethod,
    root_folder_id: String,
}

enum DriveAuthMethod {
    ServiceAccount(ServiceAccountCredentials),
    OAuth {
        client_id: String,
        client_secret: String,
        refresh_token: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct ServiceAccountCredentials {
    client_email: String,
    private_key: String,
    #[serde(default = "default_token_uri")]
    token_uri: String,
}

#[derive(Debug, Serialize)]
struct JwtClaims {
    iss: String,
    scope: String,
    aud: String,
    exp: i64,
    iat: i64,
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

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
    id: String,
    #[serde(rename = "driveId", default)]
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
    pub fn new(
        config_path: &Path,
        config: &GoogleDriveBackupConfig,
    ) -> std::result::Result<Self, CoolifyError> {
        let has_oauth = config
            .oauth_refresh_token
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);

        let auth = if has_oauth {
            let client_id = config
                .oauth_client_id
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| {
                    CoolifyError::Validation(
                        "OAuth configurado pero falta GOOGLE_DRIVE_OAUTH_CLIENT_ID".to_string(),
                    )
                })?;
            let client_secret = config
                .oauth_client_secret
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| {
                    CoolifyError::Validation(
                        "OAuth configurado pero falta GOOGLE_DRIVE_OAUTH_CLIENT_SECRET".to_string(),
                    )
                })?;
            DriveAuthMethod::OAuth {
                client_id: client_id.to_string(),
                client_secret: client_secret.to_string(),
                refresh_token: config.oauth_refresh_token.as_ref().unwrap().clone(),
            }
        } else {
            let credentials_path = resolve_credentials_path(config_path, &config.credentials_path);
            let raw = fs::read_to_string(&credentials_path)?;
            let credentials: ServiceAccountCredentials =
                serde_json::from_str(&raw).map_err(|error| {
                    CoolifyError::Validation(format!(
                        "Credenciales Google Drive invalidas '{}': {error}",
                        credentials_path.display()
                    ))
                })?;
            DriveAuthMethod::ServiceAccount(credentials)
        };

        Ok(Self {
            client: Client::new(),
            auth,
            root_folder_id: config.root_folder_id.clone(),
        })
    }

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

        /* Con OAuth las service accounts no necesitan Shared Drive: la cuota usa la del usuario autenticado */
        if matches!(self.auth, DriveAuthMethod::ServiceAccount(_)) && metadata.drive_id.is_none() {
            return Err(CoolifyError::Validation(format!(
                "La carpeta '{}' no esta en una Shared Drive. Las service accounts sin cuota no pueden subir a My Drive. Usa OAuth (auth-drive) o una Shared Drive",
                self.root_folder_id
            )));
        }

        if !metadata
            .capabilities
            .as_ref()
            .map(|value| value.can_add_children)
            .unwrap_or(false)
        {
            return Err(CoolifyError::Validation(format!(
                "Sin permisos de escritura sobre la carpeta Google Drive '{}'",
                self.root_folder_id
            )));
        }

        Ok(())
    }

    /* Flujo OAuth: intercambia un authorization code por tokens */
    pub async fn exchange_auth_code(
        client_id: &str,
        client_secret: &str,
        code: &str,
        redirect_uri: &str,
    ) -> std::result::Result<(String, String), CoolifyError> {
        let client = Client::new();
        let response = client
            .post(GOOGLE_TOKEN_URL)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("redirect_uri", redirect_uri),
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
            return Err(CoolifyError::Validation(format!(
                "OAuth token exchange fallo ({status}): {body}"
            )));
        }

        let token_response: OAuthTokenResponse = serde_json::from_str(&body)
            .map_err(|error| ApiError::InvalidResponse(error.to_string()))?;

        let refresh_token = token_response.refresh_token.ok_or_else(|| {
            CoolifyError::Validation("Google no devolvio refresh_token. Revoca el acceso en https://myaccount.google.com/permissions y reintenta".to_string())
        })?;

        Ok((token_response.access_token, refresh_token))
    }

    pub fn build_oauth_url(client_id: &str, redirect_uri: &str) -> String {
        format!(
            "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent",
            urlencoding(client_id),
            urlencoding(redirect_uri),
            urlencoding(DRIVE_SCOPE),
        )
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
            escape_query_literal(&tier_folder.id),
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
            escape_query_literal(name),
            escape_query_literal(parent_id)
        );
        if let Some(mime_type) = mime_type {
            query.push_str(&format!(
                " and mimeType = '{}'",
                escape_query_literal(mime_type)
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

    async fn access_token(&self) -> std::result::Result<String, CoolifyError> {
        match &self.auth {
            DriveAuthMethod::ServiceAccount(credentials) => self.access_token_sa(credentials).await,
            DriveAuthMethod::OAuth {
                client_id,
                client_secret,
                refresh_token,
            } => {
                Self::access_token_oauth(&self.client, client_id, client_secret, refresh_token)
                    .await
            }
        }
    }

    async fn access_token_sa(
        &self,
        credentials: &ServiceAccountCredentials,
    ) -> std::result::Result<String, CoolifyError> {
        let now = Utc::now();
        let claims = JwtClaims {
            iss: credentials.client_email.clone(),
            scope: DRIVE_SCOPE.to_string(),
            aud: credentials.token_uri.clone(),
            exp: (now + Duration::minutes(50)).timestamp(),
            iat: now.timestamp(),
        };

        let jwt = jsonwebtoken::encode(
            &Header::new(Algorithm::RS256),
            &claims,
            &EncodingKey::from_rsa_pem(credentials.private_key.as_bytes()).map_err(|error| {
                CoolifyError::Validation(format!("Clave privada Google invalida: {error}"))
            })?,
        )
        .map_err(|error| {
            CoolifyError::Validation(format!("No se pudo firmar JWT Google: {error}"))
        })?;

        let response = self
            .client
            .post(&credentials.token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", jwt.as_str()),
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

        let token: OAuthTokenResponse = serde_json::from_str(&body)
            .map_err(|error| ApiError::InvalidResponse(error.to_string()))?;
        Ok(token.access_token)
    }

    async fn access_token_oauth(
        client: &Client,
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> std::result::Result<String, CoolifyError> {
        let response = client
            .post(GOOGLE_TOKEN_URL)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", client_id),
                ("client_secret", client_secret),
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
            return Err(CoolifyError::Validation(format!(
                "OAuth refresh token fallo ({status}): {body}. Reautoriza con 'auth-drive'"
            )));
        }

        let token: OAuthTokenResponse = serde_json::from_str(&body)
            .map_err(|error| ApiError::InvalidResponse(error.to_string()))?;
        Ok(token.access_token)
    }

    fn auth_identity(&self) -> String {
        match &self.auth {
            DriveAuthMethod::ServiceAccount(credentials) => credentials.client_email.clone(),
            DriveAuthMethod::OAuth { .. } => "la cuenta OAuth autorizada".to_string(),
        }
    }
}

fn resolve_credentials_path(config_path: &Path, credentials_path: &str) -> PathBuf {
    let candidate = PathBuf::from(credentials_path);
    if candidate.is_absolute() {
        return candidate;
    }

    let config_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
    let relative_to_config = config_dir.join(&candidate);
    if relative_to_config.exists() {
        return relative_to_config;
    }

    let project_root = config_dir.parent().unwrap_or(config_dir);
    project_root.join(candidate)
}

fn escape_query_literal(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

fn urlencoding(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

fn default_token_uri() -> String {
    GOOGLE_TOKEN_URL.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_query_literal() {
        assert_eq!(escape_query_literal("o'hara"), "o\\'hara");
    }

    #[test]
    fn test_resolve_credentials_path_relative_to_config() {
        let config_path = Path::new("C:/tmp/app/config/settings.json");
        let resolved = resolve_credentials_path(config_path, "service-account.json");
        assert!(resolved.ends_with("app/service-account.json"));
    }

    #[test]
    fn test_urlencoding_basic() {
        assert_eq!(urlencoding("hello world"), "hello%20world");
        assert_eq!(urlencoding("a/b"), "a%2Fb");
    }
}
