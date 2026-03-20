use crate::config::{DnsProviderKind, Settings};
use crate::domain::{DnsRecordType, SiteConfig, SiteDnsConfig, SiteDnsRecord, StackTemplate};
use crate::error::CoolifyError;
use crate::infra::contabo_api::{ContaboApiClient, ContaboDnsRecordPayload};

use reqwest::Url;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DnsSwitchAction {
    pub record_name: String,
    pub record_type: String,
    pub action: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DnsSwitchReport {
    pub provider: String,
    pub zone: String,
    pub target_ip: String,
    pub dry_run: bool,
    pub actions: Vec<DnsSwitchAction>,
}

pub async fn switch_site_dns(
    settings: &Settings,
    site: &SiteConfig,
    target_ip: &str,
    dry_run: bool,
) -> std::result::Result<DnsSwitchReport, CoolifyError> {
    if site.template == StackTemplate::Minecraft {
        return Err(CoolifyError::Validation(
            "Minecraft queda fuera del failover DNS automático".to_string(),
        ));
    }

    let dns_config = site.dns_config.as_ref().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{}' sin dnsConfig", site.nombre))
    })?;
    let provider = settings.get_dns_provider(&dns_config.provider)?;

    match &provider.provider {
        DnsProviderKind::Contabo(contabo) => {
            let client = ContaboApiClient::new(contabo)?;
            let existing = client.list_dns_zone_records(&dns_config.zone).await?;
            let desired_records = resolve_records_for_site(site, dns_config)?;
            let mut actions = Vec::new();

            for record in desired_records {
                let record_name = normalize_record_name(&record.name);
                let matches: Vec<_> = existing
                    .iter()
                    .filter(|candidate| {
                        normalize_record_name(&candidate.name) == record_name
                            && candidate
                                .record_type
                                .eq_ignore_ascii_case(&record.record_type.to_string())
                    })
                    .collect();

                if matches.len() > 1 {
                    return Err(CoolifyError::Validation(format!(
                        "La zona '{}' tiene múltiples registros {} {} y la actualización sería ambigua",
                        dns_config.zone, record.record_type, printable_record_name(&record_name)
                    )));
                }

                let payload = ContaboDnsRecordPayload {
                    name: if record_name == "@" {
                        String::new()
                    } else {
                        record_name.clone()
                    },
                    record_type: record.record_type.to_string(),
                    ttl: record.ttl,
                    prio: 0,
                    data: target_ip.to_string(),
                };

                match matches.first() {
                    Some(existing_record)
                        if existing_record.data == target_ip
                            && existing_record.ttl == record.ttl =>
                    {
                        actions.push(DnsSwitchAction {
                            record_name: printable_record_name(&record_name),
                            record_type: record.record_type.to_string(),
                            action: "unchanged".to_string(),
                            value: target_ip.to_string(),
                        });
                    }
                    Some(existing_record) => {
                        actions.push(DnsSwitchAction {
                            record_name: printable_record_name(&record_name),
                            record_type: record.record_type.to_string(),
                            action: if dry_run { "would-update" } else { "updated" }.to_string(),
                            value: target_ip.to_string(),
                        });
                        if !dry_run {
                            client
                                .update_dns_zone_record(
                                    &dns_config.zone,
                                    existing_record.id,
                                    &payload,
                                )
                                .await?;
                        }
                    }
                    None => {
                        actions.push(DnsSwitchAction {
                            record_name: printable_record_name(&record_name),
                            record_type: record.record_type.to_string(),
                            action: if dry_run { "would-create" } else { "created" }.to_string(),
                            value: target_ip.to_string(),
                        });
                        if !dry_run {
                            client
                                .create_dns_zone_record(&dns_config.zone, &payload)
                                .await?;
                        }
                    }
                }
            }

            Ok(DnsSwitchReport {
                provider: provider.name.clone(),
                zone: dns_config.zone.clone(),
                target_ip: target_ip.to_string(),
                dry_run,
                actions,
            })
        }
    }
}

fn resolve_records_for_site(
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

fn relative_record_from_host(host: &str, zone: &str) -> std::result::Result<String, CoolifyError> {
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

fn normalize_record_name(name: &str) -> String {
    let trimmed = name.trim().trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "@" {
        "@".to_string()
    } else {
        trimmed.to_string()
    }
}

fn printable_record_name(name: &str) -> String {
    if name == "@" {
        "@".to_string()
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{BackupPolicy, HealthCheckConfig};

    fn sample_site(domain: &str, template: StackTemplate) -> SiteConfig {
        SiteConfig {
            nombre: "blog".to_string(),
            dominio: domain.to_string(),
            target: None,
            stack_uuid: Some("stack".to_string()),
            glory_branch: "main".to_string(),
            library_branch: "main".to_string(),
            theme_name: "glory".to_string(),
            skip_react: false,
            template,
            php_config: None,
            smtp_config: None,
            disable_wp_cron: false,
            backup_policy: BackupPolicy::default(),
            health_check: HealthCheckConfig::default(),
            dns_config: None,
        }
    }

    #[test]
    fn test_relative_record_from_host() {
        assert_eq!(
            relative_record_from_host("kamples.com", "kamples.com").unwrap(),
            "@"
        );
        assert_eq!(
            relative_record_from_host("task.nakomi.studio", "nakomi.studio").unwrap(),
            "task"
        );
        assert_eq!(
            relative_record_from_host("ws.task.nakomi.studio", "nakomi.studio").unwrap(),
            "ws.task"
        );
    }

    #[test]
    fn test_resolve_records_for_kamples_adds_ws() {
        let site = sample_site("https://kamples.com", StackTemplate::Kamples);
        let dns = SiteDnsConfig {
            provider: "contabo".to_string(),
            zone: "kamples.com".to_string(),
            switch_on_migration: true,
            records: Vec::new(),
        };
        let records = resolve_records_for_site(&site, &dns).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].name, "@");
        assert_eq!(records[1].name, "ws");
    }
}
