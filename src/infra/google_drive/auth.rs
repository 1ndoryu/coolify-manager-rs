use crate::error::{ApiError, CoolifyError};

use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::GoogleDriveClient;

const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/* [014A-20] Dual auth: OAuth → Service Account fallback.
 * Si OAuth falla con invalid_grant (token expirado), se intenta SA automáticamente.
 * SA requiere que rootFolderId esté en Shared Drive o compartido con la service account.
 * Este fallback evita que backups automáticos (Task Scheduler) fallen silenciosamente
 * cuando el refresh token expira y no hay usuario para re-autenticar. */
pub(super) enum DriveAuthMethod {
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
pub(super) struct ServiceAccountCredentials {
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

impl GoogleDriveClient {
    pub(super) async fn access_token(&self) -> std::result::Result<String, CoolifyError> {
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
            DriveAuthMethod::DualAuth {
                oauth_client_id,
                oauth_client_secret,
                oauth_refresh_token,
                service_account,
            } => {
                /* Intentar OAuth primero; si el token expiró, fallback a SA */
                match Self::access_token_oauth(
                    &self.client,
                    oauth_client_id,
                    oauth_client_secret,
                    oauth_refresh_token,
                )
                .await
                {
                    Ok(token) => Ok(token),
                    Err(oauth_error) => {
                        let error_str = oauth_error.to_string();
                        if error_str.contains("invalid_grant")
                            || error_str.contains("expired")
                            || error_str.contains("revoked")
                        {
                            tracing::warn!(
                                "OAuth token expirado, intentando service account: {error_str}"
                            );
                            self.access_token_sa(service_account).await.map_err(|sa_error| {
                                CoolifyError::Validation(format!(
                                    "OAuth fallo ({error_str}) y SA tambien fallo ({sa_error}). Reautoriza con 'auth-drive' o comparte la carpeta con la service account"
                                ))
                            })
                        } else {
                            Err(oauth_error)
                        }
                    }
                }
            }
        }
    }

    pub(super) async fn access_token_sa(
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

    pub(super) async fn access_token_oauth(
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
            super::urlencoding(client_id),
            super::urlencoding(redirect_uri),
            super::urlencoding(DRIVE_SCOPE),
        )
    }

    pub(super) fn auth_identity(&self) -> String {
        match &self.auth {
            DriveAuthMethod::ServiceAccount(credentials) => credentials.client_email.clone(),
            DriveAuthMethod::OAuth { .. } => "la cuenta OAuth autorizada".to_string(),
            DriveAuthMethod::DualAuth {
                service_account, ..
            } => {
                format!("OAuth (o SA fallback: {})", service_account.client_email)
            }
        }
    }
}

fn default_token_uri() -> String {
    GOOGLE_TOKEN_URL.to_string()
}