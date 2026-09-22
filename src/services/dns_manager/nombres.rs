/*
 * DNS manager — nombres: normalizacion, relativizacion y resolucion de
 * registros deseados + IP de la VPS del sitio.
 * (split 119A-5 de dns_manager.rs; codigo verbatim del original.)
 */

use crate::config::Settings;
use crate::domain::{DnsRecordType, SiteConfig, SiteDnsConfig, SiteDnsRecord, StackTemplate};
use crate::error::CoolifyError;

use reqwest::Url;

pub(crate) fn resolve_records_for_site(
    site: &SiteConfig,
    dns_config: &SiteDnsConfig,
) -> std::result::Result<Vec<SiteDnsRecord>, CoolifyError> {
    if !dns_config.records.is_empty() {
        return Ok(dns_config.records.clone());
    }

    let host = Url::parse(&site.dominio)
        .map_err(|error| {
            CoolifyError::Validation(format!("Dominio inválido '{}': {error}", site.dominio))
        })?
        .host_str()
        .ok_or_else(|| CoolifyError::Validation(format!("Dominio '{}' sin host", site.dominio)))?
        .to_string();
    let zone = dns_config.zone.trim_end_matches('.');
    let site_record = SiteDnsRecord {
        name: relative_record_from_host(&host, zone)?,
        record_type: DnsRecordType::A,
        ttl: 300,
    };

    let mut records = vec![site_record];
    if site.template == StackTemplate::Kamples {
        let ws_host = format!("ws.{host}");
        records.push(SiteDnsRecord {
            name: relative_record_from_host(&ws_host, zone)?,
            record_type: DnsRecordType::A,
            ttl: 300,
        });
    }
    Ok(records)
}

pub(crate) fn relative_record_from_host(
    host: &str,
    zone: &str,
) -> std::result::Result<String, CoolifyError> {
    if host == zone {
        return Ok("@".to_string());
    }
    let suffix = format!(".{zone}");
    if host.ends_with(&suffix) {
        return Ok(host.trim_end_matches(&suffix).to_string());
    }
    Err(CoolifyError::Validation(format!(
        "El host '{}' no pertenece a la zona '{}'",
        host, zone
    )))
}

/* Cloudflare devuelve FQDN; se relativiza a la zona para comparar. Si el
 * registro no pertenece a la zona se conserva el FQDN (no coincidirá con
 * ningún deseado y quedará como "absent", nunca se borra por error). */
pub(crate) fn relativizar_cf(fqdn: &str, zone: &str) -> String {
    relative_record_from_host(fqdn, zone).unwrap_or_else(|_| fqdn.to_string())
}

pub(crate) fn normalize_record_name(name: &str) -> String {
    let trimmed = name.trim().trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "@" {
        "@".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn printable_record_name(name: &str) -> String {
    if name == "@" {
        "@".to_string()
    } else {
        name.to_string()
    }
}

/* IP de la VPS que sirve al sitio (target propio o VPS global). */
pub(crate) fn vps_ip_para_sitio(settings: &Settings, site: &SiteConfig) -> String {
    match site.target.as_deref() {
        Some(target_name) => settings
            .targets
            .iter()
            .find(|t| t.name == target_name)
            .map(|t| t.vps.ip.clone())
            .unwrap_or_else(|| settings.vps.ip.clone()),
        None => settings.vps.ip.clone(),
    }
}
