/*
 * Comando: list-sites
 * Lista todos los sitios configurados con su estado.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;

use std::collections::HashMap;
use std::path::Path;

pub async fn execute(
    config_path: &Path,
    detailed: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;

    if settings.sitios.is_empty() {
        println!("No hay sitios configurados.");
        return Ok(());
    }

    /* Obtener estado real de los servicios si se pide detalle */
    let mut services_by_target: HashMap<String, Vec<crate::domain::ServiceInfo>> = HashMap::new();
    if detailed {
        let mut target_names = vec!["default".to_string()];
        for site in &settings.sitios {
            if let Some(target) = &site.target {
                if !target_names.iter().any(|existing| existing == target) {
                    target_names.push(target.clone());
                }
            }
        }

        for target_name in target_names {
            let target = if target_name == "default" {
                settings.default_target()
            } else {
                settings.get_target(&target_name)?.clone()
            };
            let api = CoolifyApiClient::new(&target.coolify)?;
            match api.get_services().await {
                Ok(services) => {
                    services_by_target.insert(target.name.clone(), services);
                }
                Err(e) => {
                    tracing::warn!("No se pudo obtener estado de servicios para '{}': {e}", target.name);
                    services_by_target.insert(target.name.clone(), vec![]);
                }
            }
        }
    }

    println!("{:<15} {:<35} {:<12} {:<25} {}", "NOMBRE", "DOMINIO", "TARGET", "STACK UUID", if detailed { "ESTADO" } else { "" });
    println!("{}", "-".repeat(if detailed { 105 } else { 90 }));

    for site in &settings.sitios {
        let uuid_display = site.stack_uuid.as_deref().unwrap_or("(sin asignar)");
        let target_name = site.target.as_deref().unwrap_or("default");

        let status = if detailed {
            services_by_target
                .get(target_name)
                .map(|services| services.as_slice())
                .unwrap_or(&[])
                .iter()
                .find(|s| Some(s.uuid.as_str()) == site.stack_uuid.as_deref())
                .map(|s| s.status.as_str())
                .unwrap_or("desconocido")
        } else {
            ""
        };

        println!("{:<15} {:<35} {:<12} {:<25} {}", site.nombre, site.dominio, target_name, uuid_display, status);
    }

    /* Minecraft servers si existen */
    if !settings.minecraft.is_empty() {
        println!("\n--- Servidores Minecraft ---");
        for mc in &settings.minecraft {
            let uuid = mc.stack_uuid.as_deref().unwrap_or("(sin asignar)");
            println!("  {} ({}RAM, {}p) uuid={}", mc.server_name, mc.memory, mc.max_players, uuid);
        }
    }

    println!("\nTotal: {} sitio(s), {} minecraft", settings.sitios.len(), settings.minecraft.len());
    Ok(())
}
