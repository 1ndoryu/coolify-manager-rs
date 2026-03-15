use crate::config::ContaboDnsConfig;
use crate::error::{ApiError, CoolifyError};

use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use uuid::Uuid;

const CONTABO_TIMEOUT: Duration = Duration::from_secs(30);

pub struct ContaboApiClient {
    client: Client,
    config: ContaboDnsConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct ContaboTokenResponse {
    access_token: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ContaboDataResponse<T> {
    data: Vec<T>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContaboDnsRecord {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub ttl: u32,
    #[serde(default)]
    pub prio: u32,
    pub data: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContaboDnsRecordPayload {
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub ttl: u32,
    pub prio: u32,
    pub data: String,
}

impl ContaboApiClient {
    pub fn new(config: &ContaboDnsConfig) -> std::result::Result<Self, CoolifyError> {
        let client = Client::builder()
            .timeout(CONTABO_TIMEOUT)
            .build()
            .map_err(|error| ApiError::Network(error.to_string()))?;
        Ok(Self {
            client,
            config: config.clone(),
        })
    }

    pub async fn list_dns_zone_records(
        &self,
        zone_name: &str,
    ) -> std::result::Result<Vec<ContaboDnsRecord>, CoolifyError> {
        let path = format!("/v1/dns/zones/{zone_name}/records");
        let response: ContaboDataResponse<ContaboDnsRecord> = self.request(Method::GET, &path, None::<&Value>).await?;
        Ok(response.data)
    }

    pub async fn create_dns_zone_record(
        &self,
        zone_name: &str,
        payload: &ContaboDnsRecordPayload,
    ) -> std::result::Result<(), CoolifyError> {
        let path = format!("/v1/dns/zones/{zone_name}/records");
        let _: Value = self.request(Method::POST, &path, Some(payload)).await?;
        Ok(())
    }

    pub async fn update_dns_zone_record(
        &self,
        zone_name: &str,
        record_id: i64,
        payload: &ContaboDnsRecordPayload,
    ) -> std::result::Result<(), CoolifyError> {
        let path = format!("/v1/dns/zones/{zone_name}/records/{record_id}");
        let _: Value = self.request(Method::PATCH, &path, Some(payload)).await?;
        Ok(())
    }

    async fn authenticate(&self) -> std::result::Result<String, CoolifyError> {
        let response = self
            .client
            .post(&self.config.auth_base_url)
            .form(&[
                ("grant_type", "password"),
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.as_str()),
                ("username", self.config.username.as_str()),
                ("password", self.config.api_password.as_str()),
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

        let token: ContaboTokenResponse = serde_json::from_str(&body)
            .map_err(|error| ApiError::InvalidResponse(error.to_string()))?;
        Ok(token.access_token)
    }

    async fn request<T: for<'de> Deserialize<'de>, B: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> std::result::Result<T, CoolifyError> {
        let token = self.authenticate().await?;
        let url = format!("{}{}", self.config.api_base_url.trim_end_matches('/'), path);
        let mut request = self
            .client
            .request(method, &url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("x-request-id", Uuid::new_v4().to_string())
            .header("x-trace-id", Uuid::new_v4().to_string());
        if let Some(payload) = body {
            request = request.json(payload);
        }

        let response = request
            .send()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;

        if !status.is_success() {
            return Err(ApiError::HttpError {
                status: status.as_u16(),
                body: text,
            }
            .into());
        }

        serde_json::from_str(&text).map_err(|error| ApiError::InvalidResponse(error.to_string()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contabo_client_creation() {
        let config = ContaboDnsConfig {
            client_id: "client".to_string(),
            client_secret: "secret".to_string(),
            username: "user@example.com".to_string(),
            api_password: "Password123!!".to_string(),
            api_base_url: "https://api.contabo.com".to_string(),
            auth_base_url: "https://auth.contabo.com/auth/realms/contabo/protocol/openid-connect/token".to_string(),
        };

        assert!(ContaboApiClient::new(&config).is_ok());
    }
}