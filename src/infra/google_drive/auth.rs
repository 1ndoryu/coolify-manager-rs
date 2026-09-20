/* Auth OAuth/ServiceAccount: token, identidad, URLs (split WIP de google_drive/mod.rs). */

use super::*;
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
