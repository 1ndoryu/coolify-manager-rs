/*
 * DNS manager — borrado: plan puro + ejecucion por proveedor + entradas
 * delete_site_dns / delete_orphan_dns.
 * (split 119A-5 de dns_manager.rs; codigo verbatim del original.)
 */

/* [B4-4] Borrado DNS al eliminar un sitio (test B4 20-09: `A cm-test-b4` quedó
 * huérfano apuntando a una IP sin stack detrás). Reglas de seguridad:
 * - Solo se borra el registro si su contenido == IP de la VPS del stack.
 *   Si apunta a otra IP (migración en curso) se CONSERVA y se reporta.
 * - Múltiples registros idénticos (nombre+tipo) = ambiguo: no se toca nada.
 * - Un fallo de la API aborta con error (fail-closed): reintroducir un
 *   residuo silencioso es justo lo que se corrige. El llamador decide si
 *   ese error bloquea el flujo (delete-dns) o avisa y continúa (delete-site,
 *   donde el stack ya no existe y bloquear dejaría settings inconsistente). */

use super::nombres::{
    normalize_record_name, printable_record_name, relative_record_from_host, relativizar_cf,
    resolve_records_for_site, vps_ip_para_sitio,
};
use super::tipos::{DecisionBorrado, DnsDeleteAction, DnsDeleteReport, RegistroExistente};
use crate::config::{DnsProviderKind, Settings};
use crate::domain::{DnsRecordType, SiteConfig, SiteDnsRecord};
use crate::error::CoolifyError;
use crate::infra::cloudflare_api::CloudflareApiClient;
use crate::infra::contabo_api::ContaboApiClient;

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
