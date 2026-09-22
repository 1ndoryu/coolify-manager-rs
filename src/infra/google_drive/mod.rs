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

/* [014A-20] Dual auth: OAuth → Service Account fallback.
 * Si OAuth falla con invalid_grant (token expirado), se intenta SA automáticamente.
 * SA requiere que rootFolderId esté en Shared Drive o compartido con la service account.
 * Este fallback evita que backups automáticos (Task Scheduler) fallen silenciosamente
 * cuando el refresh token expira y no hay usuario para re-autenticar. */
enum DriveAuthMethod {
    ServiceAccount(ServiceAccountCredentials),
    OAuth {
        client_id: String,
        client_secret: String,
        refresh_token: String,
    },
    DualAuth {
        oauth_client_id: String,
        oauth_client_secret: String,
        oauth_refresh_token: String,
        service_account: ServiceAccountCredentials,
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

mod archivos;
mod auth;

fn resolve_credentials_path(config_path: &Path, credentials_path: &str) -> PathBuf {
    /* [119A-5 Lote A] traversal: rechaza `..`/nul y canonicalize + starts_with
     * cuando el candidato existe; el path de credencial viene del config. */
    if credentials_path.contains("..") || credentials_path.contains('\0') {
        return PathBuf::from(credentials_path);
    }
    let candidate = PathBuf::from(credentials_path);
    if candidate.is_absolute() {
        return candidate;
    }

    let config_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
    let relative_to_config = config_dir.join(&candidate);
    if relative_to_config.exists() {
        if let (Ok(base), Ok(canon)) =
            (config_dir.canonicalize(), relative_to_config.canonicalize())
        {
            if canon.starts_with(&base) {
                return canon;
            }
        }
        return relative_to_config;
    }

    let project_root = config_dir.parent().unwrap_or(config_dir);
    let fallback = project_root.join(candidate);
    if fallback.exists() {
        if let (Ok(base), Ok(canon)) =
            (project_root.canonicalize(), fallback.canonicalize())
        {
            if canon.starts_with(&base) {
                return canon;
            }
        }
    }
    fallback
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
