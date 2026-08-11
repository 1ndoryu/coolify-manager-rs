/*
 * [257B-1] Comando env-toggle.
 * Permite cambiar rápidamente una variable de entorno en Coolify
 * sin hacer un sync-env completo. Útil para mitigación de incidentes.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::secrets;

use std::path::Path;

/// Keys que están bloqueadas para env-toggle por seguridad (matching exacto).
const BLOCKED_KEYS: &[&str] = &[
    "DATABASE_URL",
    "SERVICE_PASSWORD_POSTGRES",
    "SERVICE_USER_POSTGRES",
    "SERVICE_FQDN_POSTGRES",
    "JWT_SECRET",
    "HOST",
    "PORT",
    "STATIC_DIR",
    "SQLX_OFFLINE",
    "COOLIFY_API_TOKEN",
    "COOLIFY_BASE_URL",
    "COOLIFY_PROJECT_UUID",
    "COOLIFY_SERVER_UUID",
];

/// Substrings sensibles que bloquean una key para env-toggle.
/// Cualquier key que contenga alguno de estos (case-insensitive) se considera
/// sensible y no debe modificarse con env-toggle — usar sync-env en su lugar.
const SENSITIVE_SUBSTRINGS: &[&str] = &[
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "TOKEN",
    "CREDENTIAL",
    "PRIVATE",
    "AUTH",
];

/// Verifica si una key está bloqueada para env-toggle.
/// Usa matching exacto contra BLOCKED_KEYS + prefijos SERVICE_/COOLIFY_
/// + substrings sensibles (SECRET, PASSWORD, TOKEN, etc.).
fn is_blocked_key(key: &str) -> bool {
    let upper = key.to_uppercase();
    /* Matching exacto */
    if BLOCKED_KEYS.iter().any(|b| upper == *b) {
        return true;
    }
    /* Prefijos peligrosos */
    if upper.starts_with("SERVICE_") || upper.starts_with("COOLIFY_") {
        return true;
    }
    /* Substrings sensibles: SECRET, PASSWORD, TOKEN, etc. */
    if SENSITIVE_SUBSTRINGS.iter().any(|s| upper.contains(s)) {
        return true;
    }
    false
}

/// Cambia una variable de entorno en un servicio Coolify.
pub async fn execute(
    config_path: &Path,
    site_name: &str,
    key: &str,
    value: &str,
    restart: bool,
    dry_run: bool,
) -> Result<(), CoolifyError> {
    if key.is_empty() {
        return Err(CoolifyError::Validation(
            "La key no puede estar vacía".to_string(),
        ));
    }
    if key.contains(char::is_whitespace) || key.contains('=') {
        return Err(CoolifyError::Validation(format!(
            "La key '{}' contiene caracteres inválidos",
            key
        )));
    }
    if is_blocked_key(key) {
        return Err(CoolifyError::Validation(format!(
            "La key '{}' está bloqueada para env-toggle. Usa sync-env para variables críticas.",
            key
        )));
    }

    let settings = Settings::load(config_path)?;
    let site = settings
        .sitios
        .iter()
        .find(|s| s.nombre == site_name)
        .ok_or_else(|| CoolifyError::Validation(format!("Sitio '{}' no encontrado", site_name)))?;

    let stack_uuid = site
        .stack_uuid
        .as_deref()
        .ok_or_else(|| CoolifyError::Validation(format!("Sitio '{}' sin stackUuid", site_name)))?;

    let target_config = settings.resolve_site_target(site)?;
    let api = CoolifyApiClient::new(&target_config.coolify)?;

    /* Obtener envs actuales */
    let envs = api.get_service_envs(stack_uuid).await?;

    /* Buscar si la key ya existe */
    let existing_value = envs.iter().find_map(|e| {
        let name = e.get("name").and_then(|v| v.as_str())?;
        if name == key {
            e.get("value").and_then(|v| v.as_str()).map(String::from)
        } else {
            None
        }
    });

    if dry_run {
        let masked_new = secrets::mask_secret(value);
        if let Some(ref old) = existing_value {
            let masked_old = secrets::mask_secret(old);
            println!(
                "[DRY RUN] Actualizaría {} de '{}' a '{}'",
                key, masked_old, masked_new
            );
        } else {
            println!("[DRY RUN] Crearía {}='{}'", key, masked_new);
        }
        if restart {
            println!("[DRY RUN] Reiniciaría el servicio '{}'", site_name);
        }
        return Ok(());
    }

    /* Aplicar cambio usando push_service_envs */
    api.push_service_envs(stack_uuid, &[(key.to_string(), value.to_string())])
        .await?;

    if let Some(ref old) = existing_value {
        let masked_old = secrets::mask_secret(old);
        let masked_new = secrets::mask_secret(value);
        println!("✓ {} actualizado: '{}' → '{}'", key, masked_old, masked_new);
    } else {
        let masked_new = secrets::mask_secret(value);
        println!("✓ {} creado: '{}'", key, masked_new);
    }

    /* Reiniciar si se solicitó */
    if restart {
        println!("Reiniciando servicio '{}'...", site_name);
        api.restart_service(stack_uuid).await?;
        println!("✓ Servicio reiniciado");
    }

    Ok(())
}
