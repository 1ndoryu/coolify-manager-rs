/*
 * Cliente API de Cloudflare para gestión DNS.
 * Usa API Token (no Global API Key) para autenticación.
 * Endpoints: https://api.cloudflare.com/client/v4/
 *
 * [156A-1] Integrado para automatizar DNS + HTTPS en sitios Coolify.
 */

use crate::config::CloudflareDnsConfig;
use crate::error::{ApiError, CoolifyError};

use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const CF_TIMEOUT: Duration = Duration::from_secs(30);
const CF_BASE_URL: &str = "https://api.cloudflare.com/client/v4";

/// Cliente para la API de Cloudflare (DNS).
pub struct CloudflareApiClient {
    client: Client,
    config: CloudflareDnsConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct CfResponse<T> {
    success: bool,
    errors: Vec<CfError>,
    result: Option<T>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct CfError {
    code: u64,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CfZone {
    pub id: String,
    pub name: String,
    pub status: String,
    pub name_servers: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CfDnsRecord {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub content: String,
    pub ttl: u32,
    #[serde(default)]
    pub proxied: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CfDnsRecordPayload {
    #[serde(rename = "type")]
    pub record_type: String,
    pub name: String,
    pub content: String,
    pub ttl: u32,
    pub proxied: bool,
}

impl CloudflareApiClient {
    pub fn new(config: &CloudflareDnsConfig) -> std::result::Result<Self, CoolifyError> {
        let client = Client::builder()
            .timeout(CF_TIMEOUT)
            .build()
            .map_err(|e| ApiError::Network(e.to_string()))?;
        Ok(Self {
            client,
            config: config.clone(),
        })
    }

    /// Lista todas las zonas accesibles con el token.
    pub async fn list_zones(&self) -> std::result::Result<Vec<CfZone>, CoolifyError> {
        let url = format!("{CF_BASE_URL}/zones?per_page=50");
        let resp: CfResponse<Vec<CfZone>> = self.get(&url).await?;
        resp.result.ok_or_else(|| {
            ApiError::InvalidResponse("Cloudflare list_zones sin resultado".to_string()).into()
        })
    }

    /// Busca una zona por nombre de dominio.
    pub async fn find_zone(&self, domain: &str) -> std::result::Result<CfZone, CoolifyError> {
        let zones = self.list_zones().await?;
        let normalized = domain.trim_end_matches('.').to_lowercase();
        zones
            .into_iter()
            .find(|z| z.name.eq_ignore_ascii_case(&normalized))
            .ok_or_else(|| {
                CoolifyError::Validation(format!(
                    "Zona Cloudflare no encontrada para dominio '{domain}'"
                ))
            })
    }

    /// Lista registros DNS de una zona.
    pub async fn list_dns_records(
        &self,
        zone_id: &str,
    ) -> std::result::Result<Vec<CfDnsRecord>, CoolifyError> {
        let url = format!("{CF_BASE_URL}/zones/{zone_id}/dns_records?per_page=100");
        let resp: CfResponse<Vec<CfDnsRecord>> = self.get(&url).await?;
        resp.result.ok_or_else(|| {
            ApiError::InvalidResponse("Cloudflare list_dns_records sin resultado".to_string())
                .into()
        })
    }

    /// Crea un registro DNS.
    pub async fn create_dns_record(
        &self,
        zone_id: &str,
        payload: &CfDnsRecordPayload,
    ) -> std::result::Result<CfDnsRecord, CoolifyError> {
        let url = format!("{CF_BASE_URL}/zones/{zone_id}/dns_records");
        let resp: CfResponse<CfDnsRecord> = self.post(&url, payload).await?;
        resp.result.ok_or_else(|| {
            ApiError::InvalidResponse("Cloudflare create_dns_record sin resultado".to_string())
                .into()
        })
    }

    /// Actualiza un registro DNS existente.
    pub async fn update_dns_record(
        &self,
        zone_id: &str,
        record_id: &str,
        payload: &CfDnsRecordPayload,
    ) -> std::result::Result<CfDnsRecord, CoolifyError> {
        let url = format!("{CF_BASE_URL}/zones/{zone_id}/dns_records/{record_id}");
        let resp: CfResponse<CfDnsRecord> = self.put(&url, payload).await?;
        resp.result.ok_or_else(|| {
            ApiError::InvalidResponse("Cloudflare update_dns_record sin resultado".to_string())
                .into()
        })
    }

    /* === HTTP helpers con autenticación API Token === */

    async fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> std::result::Result<T, CoolifyError> {
        let resp = self
            .client
            .get(url)
            .bearer_auth(&self.config.api_token)
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;
        self.parse_response(resp).await
    }

    async fn post<T: serde::de::DeserializeOwned, B: Serialize>(
        &self,
        url: &str,
        body: &B,
    ) -> std::result::Result<T, CoolifyError> {
        let resp = self
            .client
            .post(url)
            .bearer_auth(&self.config.api_token)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;
        self.parse_response(resp).await
    }

    async fn put<T: serde::de::DeserializeOwned, B: Serialize>(
        &self,
        url: &str,
        body: &B,
    ) -> std::result::Result<T, CoolifyError> {
        let resp = self
            .client
            .put(url)
            .bearer_auth(&self.config.api_token)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;
        self.parse_response(resp).await
    }

    async fn parse_response<T: serde::de::DeserializeOwned>(
        &self,
        resp: reqwest::Response,
    ) -> std::result::Result<T, CoolifyError> {
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;

        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(ApiError::HttpError {
                status: status.as_u16(),
                body: "Cloudflare API token inválido o sin permisos".to_string(),
            }
            .into());
        }

        let parsed: CfResponse<T> = serde_json::from_str(&body).map_err(|e| {
            ApiError::InvalidResponse(format!("Cloudflare JSON parse error: {e} — body: {body}"))
        })?;

        if !parsed.success {
            let messages: Vec<String> = parsed.errors.iter().map(|e| e.message.clone()).collect();
            return Err(ApiError::HttpError {
                status: status.as_u16(),
                body: format!("Cloudflare API errors: {}", messages.join("; ")),
            }
            .into());
        }

        parsed.result.ok_or_else(|| {
            ApiError::InvalidResponse("Cloudflare respuesta sin campo 'result'".to_string()).into()
        })
    }
}
