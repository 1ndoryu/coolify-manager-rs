/*
 * DNS manager — switch: conmutacion de registros hacia la IP objetivo
 * (Contabo y Cloudflare).
 * (split 119A-5 de dns_manager.rs; codigo verbatim del original.)
 */

use super::nombres::{normalize_record_name, printable_record_name, resolve_records_for_site};
use super::tipos::{DnsSwitchAction, DnsSwitchReport, ObjetivoSwitch};
use crate::config::{CloudflareDnsConfig, ContaboDnsConfig, DnsProviderKind, Settings};
use crate::domain::{SiteConfig, StackTemplate};
use crate::error::CoolifyError;
use crate::infra::cloudflare_api::{CfDnsRecordPayload, CloudflareApiClient};
use crate::infra::contabo_api::{ContaboApiClient, ContaboDnsRecordPayload};

pub async fn switch_site_dns(
    settings: &Settings,
    site: &SiteConfig,
    target_ip: &str,
    dry_run: bool,
) -> std::result::Result<DnsSwitchReport, CoolifyError> {
    let (provider, objetivo) = preparar_switch(settings, site, target_ip, dry_run).await?;

    match &provider.provider {
        DnsProviderKind::Contabo(contabo) => {
            switch_contabo(contabo, &provider.name, &objetivo).await
        }
        DnsProviderKind::Cloudflare(cf_config) => {
            switch_cloudflare(cf_config, &provider.name, &objetivo).await
        }
    }
}

/* Valida template/zona/proveedor y resuelve los registros deseados. */
async fn preparar_switch(
    settings: &Settings,
    site: &SiteConfig,
    target_ip: &str,
    dry_run: bool,
) -> std::result::Result<(crate::config::DnsProviderConfig, ObjetivoSwitch), CoolifyError> {
    if site.template == StackTemplate::Minecraft {
        return Err(CoolifyError::Validation(
            "Minecraft queda fuera del failover DNS automático".to_string(),
        ));
    }

    let dns_config = site.dns_config.as_ref().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{}' sin dnsConfig", site.nombre))
    })?;
    let provider = settings.get_dns_provider(&dns_config.provider)?.clone();
    let desired_records = resolve_records_for_site(site, dns_config)?;
    let objetivo = ObjetivoSwitch {
        zone: dns_config.zone.clone(),
        target_ip: target_ip.to_string(),
        dry_run,
        desired: desired_records,
    };
    Ok((provider, objetivo))
}

/* Concilia registros en Contabo (nombre relativo, payload Contabo). */
async fn switch_contabo(
    contabo: &ContaboDnsConfig,
    provider_name: &str,
    objetivo: &ObjetivoSwitch,
) -> std::result::Result<DnsSwitchReport, CoolifyError> {
    let client = ContaboApiClient::new(contabo)?;
    let existing = client.list_dns_zone_records(&objetivo.zone).await?;
    let mut actions = Vec::new();

    for record in &objetivo.desired {
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
                objetivo.zone,
                record.record_type,
                printable_record_name(&record_name)
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
            data: objetivo.target_ip.clone(),
        };

        match matches.first() {
            Some(existing_record)
                if existing_record.data == objetivo.target_ip
                    && existing_record.ttl == record.ttl =>
            {
                actions.push(DnsSwitchAction {
                    record_name: printable_record_name(&record_name),
                    record_type: record.record_type.to_string(),
                    action: "unchanged".to_string(),
                    value: objetivo.target_ip.clone(),
                });
            }
            Some(existing_record) => {
                actions.push(DnsSwitchAction {
                    record_name: printable_record_name(&record_name),
                    record_type: record.record_type.to_string(),
                    action: if objetivo.dry_run {
                        "would-update"
                    } else {
                        "updated"
                    }
                    .to_string(),
                    value: objetivo.target_ip.clone(),
                });
                if !objetivo.dry_run {
                    client
                        .update_dns_zone_record(&objetivo.zone, existing_record.id, &payload)
                        .await?;
                }
            }
            None => {
                actions.push(DnsSwitchAction {
                    record_name: printable_record_name(&record_name),
                    record_type: record.record_type.to_string(),
                    action: if objetivo.dry_run {
                        "would-create"
                    } else {
                        "created"
                    }
                    .to_string(),
                    value: objetivo.target_ip.clone(),
                });
                if !objetivo.dry_run {
                    client
                        .create_dns_zone_record(&objetivo.zone, &payload)
                        .await?;
                }
            }
        }
    }

    Ok(DnsSwitchReport {
        provider: provider_name.to_string(),
        zone: objetivo.zone.clone(),
        target_ip: objetivo.target_ip.clone(),
        dry_run: objetivo.dry_run,
        actions,
    })
}
/* Concilia registros en Cloudflare (FQDN, payload CF con proxy). */
async fn switch_cloudflare(
    cf_config: &CloudflareDnsConfig,
    provider_name: &str,
    objetivo: &ObjetivoSwitch,
) -> std::result::Result<DnsSwitchReport, CoolifyError> {
    let client = CloudflareApiClient::new(cf_config)?;
    let zone = client.find_zone(&objetivo.zone).await?;
    let existing = client.list_dns_records(&zone.id).await?;
    let mut actions = Vec::new();

    for record in &objetivo.desired {
        let record_name = normalize_record_name(&record.name);
        let fqdn = if record_name == "@" {
            objetivo.zone.clone()
        } else {
            format!("{}.{}", record_name, objetivo.zone)
        };

        let matches: Vec<_> = existing
            .iter()
            .filter(|candidate| {
                candidate.name.eq_ignore_ascii_case(&fqdn)
                    && candidate
                        .record_type
                        .eq_ignore_ascii_case(&record.record_type.to_string())
            })
            .collect();

        if matches.len() > 1 {
            return Err(CoolifyError::Validation(format!(
                "Zona Cloudflare '{}' tiene múltiples registros {} {} — ambiguo",
                objetivo.zone,
                record.record_type,
                printable_record_name(&record_name)
            )));
        }

        let payload = CfDnsRecordPayload {
            record_type: record.record_type.to_string(),
            name: fqdn.clone(),
            content: objetivo.target_ip.clone(),
            ttl: record.ttl,
            proxied: cf_config.proxy_default && record.record_type.to_string() == "A",
        };

        match matches.first() {
            Some(existing_record)
                if existing_record.content == objetivo.target_ip
                    && existing_record.ttl == record.ttl
                    && existing_record.proxied == payload.proxied =>
            {
                actions.push(DnsSwitchAction {
                    record_name: printable_record_name(&record_name),
                    record_type: record.record_type.to_string(),
                    action: "unchanged".to_string(),
                    value: objetivo.target_ip.clone(),
                });
            }
            Some(existing_record) => {
                actions.push(DnsSwitchAction {
                    record_name: printable_record_name(&record_name),
                    record_type: record.record_type.to_string(),
                    action: if objetivo.dry_run {
                        "would-update"
                    } else {
                        "updated"
                    }
                    .to_string(),
                    value: objetivo.target_ip.clone(),
                });
                if !objetivo.dry_run {
                    client
                        .update_dns_record(&zone.id, &existing_record.id, &payload)
                        .await?;
                }
            }
            None => {
                actions.push(DnsSwitchAction {
                    record_name: printable_record_name(&record_name),
                    record_type: record.record_type.to_string(),
                    action: if objetivo.dry_run {
                        "would-create"
                    } else {
                        "created"
                    }
                    .to_string(),
                    value: objetivo.target_ip.clone(),
                });
                if !objetivo.dry_run {
                    client.create_dns_record(&zone.id, &payload).await?;
                }
            }
        }
    }

    Ok(DnsSwitchReport {
        provider: provider_name.to_string(),
        zone: objetivo.zone.clone(),
        target_ip: objetivo.target_ip.clone(),
        dry_run: objetivo.dry_run,
        actions,
    })
}
