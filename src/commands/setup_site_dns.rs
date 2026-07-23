/*
 * setup-site-dns — Configura DNS y asegura HTTPS para un sitio.
 *
 * [156A-1] Flujo:
 *   1. Lee configuración del sitio y su DNS.
 *   2. Resuelve IP del target (propia o explícita).
 *   3. Ejecuta switch_site_dns para crear/actualizar registros.
 *   4. Verifica que el dominio apunte a HTTPS.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::dns_manager;

use clap::Args;
use tracing::{info, warn};

#[derive(Args)]
pub struct SetupSiteDnsArgs {
    /// Nombre del sitio (de settings.json)
    pub name: String,

    /// IP destino explícita (omite resolución automática)
    #[arg(long)]
    pub ip: Option<String>,

    /// Solo muestra lo que haría sin ejecutar
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    /// Omite verificación HTTP post-configuración
    #[arg(long, default_value_t = false)]
    pub skip_verify: bool,
}

pub async fn run(settings: &Settings, args: &SetupSiteDnsArgs) -> Result<(), CoolifyError> {
    let site = settings
        .sitios
        .iter()
        .find(|s| s.nombre == args.name)
        .ok_or_else(|| CoolifyError::Validation(format!("Sitio '{}' no encontrado", args.name)))?;

    let dns_config = site.dns_config.as_ref().ok_or_else(|| {
        CoolifyError::Validation(format!(
            "Sitio '{}' no tiene dns_config configurado",
            args.name
        ))
    })?;

    info!(
        "Configurando DNS para sitio '{}' — zona: {}, dominio: {}",
        args.name, dns_config.zone, site.dominio
    );
    warn!("DNS config: provider='{}', zone='{}'", dns_config.provider, dns_config.zone);

    let target_ip = match &args.ip {
        Some(ip) => ip.clone(),
        None => {
            /* Si el sitio tiene un target específico, buscar en targets[];
             * si no, usar la VPS global (settings.vps). */
            match &site.target {
                Some(target_name) => {
                    settings
                        .targets
                        .iter()
                        .find(|t| &t.name == target_name)
                        .map(|t| t.vps.ip.clone())
                        .ok_or_else(|| {
                            CoolifyError::Validation(format!(
                                "No se encontró target '{}' para el sitio '{}'",
                                target_name, args.name
                            ))
                        })?
                }
                None => settings.vps.ip.clone(),
            }
        }
    };

    info!("IP destino: {target_ip}");

    let report = dns_manager::switch_site_dns(
        settings,
        site,
        &target_ip,
        args.dry_run,
    )
    .await?;

    if args.dry_run {
        println!("[dry-run] Reporte DNS:");
    } else {
        println!("Reporte DNS:");
    }
    println!("  Provider: {}", report.provider);
    println!("  Zona: {}", report.zone);
    println!("  IP destino: {}", report.target_ip);
    for action in &report.actions {
        let symbol = match action.action.as_str() {
            "unchanged" => "  =",
            "created" | "would-create" => "  +",
            "updated" | "would-update" => "  ~",
            _ => "  ?",
        };
        println!(
            "{symbol} {} {} → {} [{action}]",
            action.record_type, action.record_name, action.value,
            action = action.action
        );
    }

    if !args.dry_run && !args.skip_verify {
        let expected_url = format!("https://{}/healthz", site.dominio);
        info!("Verificando HTTPS: {expected_url}");
        match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| CoolifyError::Validation(e.to_string()))?
            .get(&expected_url)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                println!("✓ HTTPS verificado: {expected_url} respondió {}", resp.status());
            }
            Ok(resp) => {
                println!("⚠ HTTPS respondió {} — puede necesitar unos minutos tras cambio DNS", resp.status());
            }
            Err(e) => {
                println!("⚠ No se pudo verificar HTTPS: {e} — los certificados pueden tardar en propagarse");
            }
        }
    }

    Ok(())
}
