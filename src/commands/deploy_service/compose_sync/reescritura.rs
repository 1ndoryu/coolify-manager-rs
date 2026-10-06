/* Reescritura de claves del compose Rust + labels Traefik [06AA-4 split compose_sync]. */

use super::volumenes::inject_postgres_data_volume;
use crate::error::CoolifyError;

pub(super) fn rewrite_rust_service_compose(
    current_compose: &str,
    repo_url: &str,
    glory_branch: &str,
    domain: &str,
    app_bin: &str,
    frontend_dir: &str,
) -> std::result::Result<String, CoolifyError> {
    let mut compose =
        replace_compose_key_value(current_compose, "REPO_URL:", &format!("'{repo_url}'"))?;
    compose = replace_compose_key_value(&compose, "BRANCH:", glory_branch)?;
    compose = replace_compose_key_value(&compose, "APP_BIN:", app_bin)?;
    /* [268A-4] FRONTEND_DIR: directorio del frontend en el repo. Se añade al
     * compose si el template lo declara (proyectos no-glory); si no existe la
     * clave, se inserta tras APP_BIN para que el build lo reciba. */
    if compose
        .lines()
        .any(|l| l.trim_start().starts_with("FRONTEND_DIR:"))
    {
        compose = replace_compose_key_value(&compose, "FRONTEND_DIR:", frontend_dir)?;
    } else {
        let mut lines: Vec<String> = Vec::new();
        let mut inserted = false;
        for line in compose.lines() {
            if !inserted && line.trim_start().starts_with("APP_BIN:") {
                let indent = &line[..line.len() - line.trim_start().len()];
                lines.push(line.to_string());
                lines.push(format!("{indent}FRONTEND_DIR: {frontend_dir}"));
                inserted = true;
            } else {
                lines.push(line.to_string());
            }
        }
        if !inserted {
            return Err(CoolifyError::Validation(
                "Compose Rust actual no contiene la clave requerida 'APP_BIN'".into(),
            ));
        }
        compose = lines.join("\n");
    }
    compose = replace_compose_key_value(&compose, "SERVICE_FQDN_APP:", &format!("'{domain}'"))?;
    let compose = rewrite_compose_host_rules(&compose, normalize_domain_host(domain));
    /* [235A-4] Asegurar que Traefik pueda enrutar al contenedor en la red correcta.
     * Sitios legacy no tienen este label → 503 "no available server". */
    let compose = inject_traefik_network_label(&compose);
    /* [21C-7] Asegurar que postgres tiene volumen de datos persistente.
     * Stacks legacy pueden no tener pg_data montado → E18 bloquea el deploy. */
    Ok(inject_postgres_data_volume(&compose))
}

fn replace_compose_key_value(
    compose: &str,
    key: &str,
    value: &str,
) -> std::result::Result<String, CoolifyError> {
    let ends_with_newline = compose.ends_with('\n');
    let mut replaced = false;
    let mut lines = Vec::new();

    for line in compose.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(key) {
            let indent = &line[..line.len() - trimmed.len()];
            lines.push(format!("{indent}{key} {value}"));
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !replaced {
        return Err(CoolifyError::Validation(format!(
            "Compose Rust actual no contiene la clave requerida '{key}'"
        )));
    }

    let mut updated = lines.join("\n");
    if ends_with_newline {
        updated.push('\n');
    }
    Ok(updated)
}

/// [235A-4] Inyecta `traefik.docker.network=coolify` si falta.
/// Sitios legacy (creados antes de la regla) no tienen este label.
/// Sin él, Traefik no puede encontrar el contenedor en la red correcta → 503 "no available server".
/// Busca `- "traefik.enable=true"` y agrega el label justo después.
pub(crate) fn inject_traefik_network_label(compose: &str) -> String {
    if compose.contains("traefik.docker.network=coolify") {
        return compose.to_string();
    }
    let ends_with_newline = compose.ends_with('\n');
    let mut lines: Vec<String> = Vec::new();
    let mut injected = false;

    for line in compose.lines() {
        lines.push(line.to_string());
        if !injected {
            let trimmed = line.trim();
            // Detectar variaciones: con o sin comillas, con o sin guión
            if trimmed.contains("traefik.enable=true") {
                let indent = &line[..line.len() - line.trim_start().len()];
                lines.push(format!(r#"{indent}- "traefik.docker.network=coolify""#));
                injected = true;
            }
        }
    }

    let mut result = lines.join("\n");
    if ends_with_newline {
        result.push('\n');
    }
    if !injected {
        // Si no encontró traefik.enable, el compose no tiene labels Traefik.
        // Log warning pero no fallar — el compose podría ser de otro tipo.
        eprintln!(
            "[WARN] inject_traefik_network_label: no se encontró 'traefik.enable=true' en el compose. Label no inyectado."
        );
    }
    result
}

pub(super) fn rewrite_compose_host_rules(compose: &str, domain_host: &str) -> String {
    let ends_with_newline = compose.ends_with('\n');
    let mut lines = Vec::new();

    for line in compose.lines() {
        if let Some((prefix, rest)) = line.split_once("Host(") {
            if let Some(end_index) = rest.find(')') {
                /* [E4+E5 fix] Generar Host(`domain`) con backticks SIEMPRE.
                 * Además, limpiar paréntesis extra acumulados del suffix
                 * para que el reemplazo sea idempotente.
                 * Ej: Host(domain)))))) → Host(`domain`)
                 */
                let after_close = &rest[end_index..];
                let suffix = after_close.trim_start_matches(')');
                lines.push(format!("{prefix}Host(`{domain_host}`){suffix}"));
                continue;
            }
        }
        lines.push(line.to_string());
    }

    let mut updated = lines.join("\n");
    if ends_with_newline {
        updated.push('\n');
    }
    updated
}

pub(super) fn normalize_domain_host(domain: &str) -> &str {
    domain
        .trim()
        .trim_end_matches('/')
        .trim_start_matches("https://")
        .trim_start_matches("http://")
}
