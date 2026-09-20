use crate::config::{DnsProviderKind, Settings};
use crate::domain::{DnsRecordType, SiteConfig, SiteDnsConfig, SiteDnsRecord, StackTemplate};
use crate::error::CoolifyError;
use crate::infra::cloudflare_api::{CfDnsRecordPayload, CloudflareApiClient};
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
        DnsProviderKind::Cloudflare(cf_config) => {
            let client = CloudflareApiClient::new(cf_config)?;
            let zone = client.find_zone(&dns_config.zone).await?;
            let existing = client.list_dns_records(&zone.id).await?;
            let desired_records = resolve_records_for_site(site, dns_config)?;
            let mut actions = Vec::new();

            for record in desired_records {
                let record_name = normalize_record_name(&record.name);
                let fqdn = if record_name == "@" {
                    dns_config.zone.clone()
                } else {
                    format!("{}.{}", record_name, dns_config.zone)
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
                        dns_config.zone,
                        record.record_type,
                        printable_record_name(&record_name)
                    )));
                }

                let payload = CfDnsRecordPayload {
                    record_type: record.record_type.to_string(),
                    name: fqdn.clone(),
                    content: target_ip.to_string(),
                    ttl: record.ttl,
                    proxied: cf_config.proxy_default && record.record_type.to_string() == "A",
                };

                match matches.first() {
                    Some(existing_record)
                        if existing_record.content == target_ip
                            && existing_record.ttl == record.ttl
                            && existing_record.proxied == payload.proxied =>
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
                                .update_dns_record(&zone.id, &existing_record.id, &payload)
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
                            client.create_dns_record(&zone.id, &payload).await?;
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

/* [B4-4] Borrado DNS al eliminar un sitio (test B4 20-09: `A cm-test-b4` quedó
 * huérfano apuntando a una IP sin stack detrás). Reglas de seguridad:
 * - Solo se borra el registro si su contenido == IP de la VPS del stack.
 *   Si apunta a otra IP (migración en curso) se CONSERVA y se reporta.
 * - Múltiples registros idénticos (nombre+tipo) = ambiguo: no se toca nada.
 * - Un fallo de la API aborta con error (fail-closed): reintroducir un
 *   residuo silencioso es justo lo que se corrige. El llamador decide si
 *   ese error bloquea el flujo (delete-dns) o avisa y continúa (delete-site,
 *   donde el stack ya no existe y bloquear dejaría settings inconsistente). */

#[derive(Debug, Clone, Serialize)]
pub struct DnsDeleteAction {
    pub record_name: String,
    pub record_type: String,
    pub action: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DnsDeleteReport {
    pub provider: String,
    pub zone: String,
    pub vps_ip: String,
    pub dry_run: bool,
    pub actions: Vec<DnsDeleteAction>,
}

/* Vista normalizada de un registro existente (común a ambos proveedores). */
#[derive(Debug, Clone)]
pub(crate) struct RegistroExistente {
    pub nombre: String,
    pub tipo: String,
    pub contenido: String,
    pub id: String,
}

/* Decisión pura por registro deseado: acción de reporte + id a borrar. */
#[derive(Debug, Clone)]
pub(crate) struct DecisionBorrado {
    pub action: DnsDeleteAction,
    pub id_a_borrar: Option<String>,
}

pub(crate) fn plan_borrado_dns(
    deseados: &[SiteDnsRecord],
    existentes: &[RegistroExistente],
    vps_ip: &str,
    dry_run: bool,
) -> Vec<DecisionBorrado> {
    let etiqueta = if dry_run { "would-delete" } else { "deleted" };
    deseados
        .iter()
        .map(|deseado| {
            let nombre = normalize_record_name(&deseado.name);
            let tipo = deseado.record_type.to_string();
            let coincidentes: Vec<_> = existentes
                .iter()
                .filter(|c| {
                    normalize_record_name(&c.nombre) == nombre
                        && c.tipo.eq_ignore_ascii_case(&tipo)
                })
                .collect();
            match coincidentes.as_slice() {
                [] => DecisionBorrado {
                    action: DnsDeleteAction {
                        record_name: printable_record_name(&nombre),
                        record_type: tipo,
                        action: "absent".to_string(),
                        value: String::new(),
                    },
                    id_a_borrar: None,
                },
                [unico] if unico.contenido == vps_ip => DecisionBorrado {
                    action: DnsDeleteAction {
                        record_name: printable_record_name(&nombre),
                        record_type: tipo,
                        action: etiqueta.to_string(),
                        value: unico.contenido.clone(),
                    },
                    id_a_borrar: Some(unico.id.clone()),
                },
                [unico] => DecisionBorrado {
                    action: DnsDeleteAction {
                        record_name: printable_record_name(&nombre),
                        record_type: tipo,
                        action: "kept-remote".to_string(),
                        value: unico.contenido.clone(),
                    },
                    id_a_borrar: None,
                },
                varios => DecisionBorrado {
                    action: DnsDeleteAction {
                        record_name: printable_record_name(&nombre),
                        record_type: tipo,
                        action: "ambiguous-skipped".to_string(),
                        value: varios
                            .iter()
                            .map(|c| c.contenido.as_str())
                            .collect::<Vec<_>>()
                            .join(","),
                    },
                    id_a_borrar: None,
                },
            }
        })
        .collect()
}

/* IP de la VPS que sirve al sitio (target propio o VPS global). */
fn vps_ip_para_sitio(settings: &Settings, site: &SiteConfig) -> String {
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

async fn ejecutar_borrado(
    provider: &crate::config::DnsProviderConfig,
    zone: &str,
    deseados: &[SiteDnsRecord],
    vps_ip: &str,
    dry_run: bool,
) -> std::result::Result<DnsDeleteReport, CoolifyError> {
    let mut actions = Vec::new();
    match &provider.provider {
        DnsProviderKind::Contabo(contabo) => {
            let client = ContaboApiClient::new(contabo)?;
            let existentes: Vec<RegistroExistente> = client
                .list_dns_zone_records(zone)
                .await?
                .iter()
                .map(|r| RegistroExistente {
                    nombre: r.name.clone(),
                    tipo: r.record_type.clone(),
                    contenido: r.data.clone(),
                    id: r.id.to_string(),
                })
                .collect();
            for decision in plan_borrado_dns(deseados, &existentes, vps_ip, dry_run) {
                if !dry_run {
                    if let Some(id) = &decision.id_a_borrar {
                        let record_id: i64 =
                            id.parse().map_err(|_| {
                                CoolifyError::Validation(format!(
                                    "ID de registro Contabo inesperado: '{id}'"
                                ))
                            })?;
                        client.delete_dns_zone_record(zone, record_id).await?;
                    }
                }
                actions.push(decision.action);
            }
        }
        DnsProviderKind::Cloudflare(cf_config) => {
            let client = CloudflareApiClient::new(cf_config)?;
            let zona = client.find_zone(zone).await?;
            let existentes: Vec<RegistroExistente> = client
                .list_dns_records(&zona.id)
                .await?
                .iter()
                .map(|r| RegistroExistente {
                    nombre: relativizar_cf(&r.name, zone),
                    tipo: r.record_type.clone(),
                    contenido: r.content.clone(),
                    id: r.id.clone(),
                })
                .collect();
            for decision in plan_borrado_dns(deseados, &existentes, vps_ip, dry_run) {
                if !dry_run {
                    if let Some(id) = &decision.id_a_borrar {
                        client.delete_dns_record(&zona.id, id).await?;
                    }
                }
                actions.push(decision.action);
            }
        }
    }
    Ok(DnsDeleteReport {
        provider: provider.name.clone(),
        zone: zone.to_string(),
        vps_ip: vps_ip.to_string(),
        dry_run,
        actions,
    })
}

/* Cloudflare devuelve FQDN; se relativiza a la zona para comparar. Si el
 * registro no pertenece a la zona se conserva el FQDN (no coincidirá con
 * ningún deseado y quedará como "absent", nunca se borra por error). */
fn relativizar_cf(fqdn: &str, zone: &str) -> String {
    relative_record_from_host(fqdn, zone).unwrap_or_else(|_| fqdn.to_string())
}

/// Borra los registros DNS del sitio que apunten a su VPS. Requiere dnsConfig
/// (sin zona conocida no hay dónde buscar; usar `delete-dns` con zona explícita).
pub async fn delete_site_dns(
    settings: &Settings,
    site: &SiteConfig,
    dry_run: bool,
) -> std::result::Result<DnsDeleteReport, CoolifyError> {
    let dns_config = site.dns_config.as_ref().ok_or_else(|| {
        CoolifyError::Validation(format!(
            "Sitio '{}' sin dnsConfig: no se conoce la zona; el DNS debe revisarse \
             a mano o con `delete-dns --zone <zona> --name <registro>`",
            site.nombre
        ))
    })?;
    let provider = settings.get_dns_provider(&dns_config.provider)?;
    let deseados = resolve_records_for_site(site, dns_config)?;
    let vps_ip = vps_ip_para_sitio(settings, site);
    ejecutar_borrado(provider, &dns_config.zone, &deseados, &vps_ip, dry_run).await
}

/* [B4-4] Borrado de un registro huérfano sin sitio en settings (p.ej. restos de
 * B4: `A cm-test-b4.wandori.us`). El nombre acepta relativo o FQDN. */
pub async fn delete_orphan_dns(
    settings: &Settings,
    provider_name: &str,
    zone: &str,
    name: &str,
    ip: Option<&str>,
    dry_run: bool,
) -> std::result::Result<DnsDeleteReport, CoolifyError> {
    let provider = settings.get_dns_provider(provider_name)?;
    let relativo = if name == zone || name.ends_with(&format!(".{zone}")) {
        relative_record_from_host(name, zone)?
    } else {
        normalize_record_name(name)
    };
    let deseados = vec![SiteDnsRecord {
        name: relativo,
        record_type: DnsRecordType::A,
        ttl: 300,
    }];
    let vps_ip = ip.map(str::to_string).unwrap_or_else(|| settings.vps.ip.clone());
    ejecutar_borrado(provider, zone, &deseados, &vps_ip, dry_run).await
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
            extra_domains: Vec::new(),
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
            repo_url: None,
            app_bin: crate::domain::default_app_bin(),
            frontend_dir: crate::domain::default_frontend_dir(),
            image_ref: None,
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

    /* [B4-4] Solo se borra si el contenido == IP de la VPS. */
    #[test]
    fn test_plan_borrado_solo_si_apunta_a_la_vps() {
        use crate::domain::DnsRecordType;
        let deseados = vec![SiteDnsRecord {
            name: "cm-test-b4".to_string(),
            record_type: DnsRecordType::A,
            ttl: 300,
        }];
        let reg = |contenido: &str| RegistroExistente {
            nombre: "cm-test-b4".to_string(),
            tipo: "A".to_string(),
            contenido: contenido.to_string(),
            id: "7".to_string(),
        };
        // Apunta a la VPS → se borra (o would-delete en dry-run).
        let plan = plan_borrado_dns(&deseados, &[reg("66.94.100.241")], "66.94.100.241", false);
        assert_eq!(plan[0].action.action, "deleted");
        assert_eq!(plan[0].id_a_borrar.as_deref(), Some("7"));
        let plan = plan_borrado_dns(&deseados, &[reg("66.94.100.241")], "66.94.100.241", true);
        assert_eq!(plan[0].action.action, "would-delete");
        assert_eq!(plan[0].id_a_borrar.as_deref(), Some("7"));
        // Apunta a otra IP (migración) → se conserva.
        let plan = plan_borrado_dns(&deseados, &[reg("9.9.9.9")], "66.94.100.241", false);
        assert_eq!(plan[0].action.action, "kept-remote");
        assert_eq!(plan[0].action.value, "9.9.9.9");
        assert!(plan[0].id_a_borrar.is_none());
        // Ausente → absent, sin id.
        let plan = plan_borrado_dns(&deseados, &[], "66.94.100.241", false);
        assert_eq!(plan[0].action.action, "absent");
        assert!(plan[0].id_a_borrar.is_none());
        // Duplicado → ambiguo, no se toca.
        let plan = plan_borrado_dns(
            &deseados,
            &[reg("66.94.100.241"), reg("66.94.100.241")],
            "66.94.100.241",
            false,
        );
        assert_eq!(plan[0].action.action, "ambiguous-skipped");
        assert!(plan[0].id_a_borrar.is_none());
    }
}
