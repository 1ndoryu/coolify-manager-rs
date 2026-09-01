use crate::config::GoogleDriveBackupConfig;
use crate::error::CoolifyError;

use auth::{DriveAuthMethod, ServiceAccountCredentials};
use reqwest::Client;
use std::fs;
use std::path::{Path, PathBuf};

mod auth;
mod files;

pub struct GoogleDriveClient {
    client: Client,
    auth: DriveAuthMethod,
    root_folder_id: String,
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

        /* Intentar cargar service account credentials (puede no existir) */
        let sa_credentials = if !config.credentials_path.is_empty() {
            let credentials_path = resolve_credentials_path(config_path, &config.credentials_path);
            match fs::read_to_string(&credentials_path) {
                Ok(raw) => match serde_json::from_str::<ServiceAccountCredentials>(&raw) {
                    Ok(creds) => Some(creds),
                    Err(error) => {
                        tracing::warn!(
                            "Credenciales SA invalidas '{}': {error}",
                            credentials_path.display()
                        );
                        None
                    }
                },
                Err(_) => None,
            }
        } else {
            None
        };

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
            let refresh_token = config
                .oauth_refresh_token
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| {
                    CoolifyError::Validation(
                        "OAuth configurado pero falta GOOGLE_DRIVE_OAUTH_REFRESH_TOKEN".to_string(),
                    )
                })?
                .to_string();

            /* Si hay SA disponible, usar DualAuth para fallback automático */
            if let Some(sa) = sa_credentials {
                DriveAuthMethod::DualAuth {
                    oauth_client_id: client_id.to_string(),
                    oauth_client_secret: client_secret.to_string(),
                    oauth_refresh_token: refresh_token,
                    service_account: sa,
                }
            } else {
                DriveAuthMethod::OAuth {
                    client_id: client_id.to_string(),
                    client_secret: client_secret.to_string(),
                    refresh_token,
                }
            }
        } else if let Some(sa) = sa_credentials {
            DriveAuthMethod::ServiceAccount(sa)
        } else {
            return Err(CoolifyError::Validation(
                "Sin credenciales Google Drive: necesita OAuth (auth-drive) o service account (credentialsPath)".to_string(),
            ));
        };

        Ok(Self {
            client: Client::new(),
            auth,
            root_folder_id: config.root_folder_id.clone(),
        })
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