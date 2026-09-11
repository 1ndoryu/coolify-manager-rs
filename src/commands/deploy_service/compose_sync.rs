use super::compose_backup::backup_compose_locally;
use super::compose_validation::{validate_compose_before_deploy, validate_postgres_creds_stable};
use super::env_building::normalize_health_path;
use crate::config::CoolifyConfig;
use crate::domain::SiteConfig;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::template_engine;
use std::path::Path;

/* Renderiza el template y lo envia a Coolify via API PATCH */
pub(crate) async fn sync_compose(
    config_path: &Path,
    site: &SiteConfig,
    stack_uuid: &str,
    coolify_config: &CoolifyConfig,
) -> std::result::Result<(), CoolifyError> {
    let api = CoolifyApiClient::new(coolify_config)?;

    /* [265A-6] Coolify acepta el compose Rust canónico del servicio (dockerfile + args),
     * pero rechaza en PATCH el template grande de creación con dockerfile_inline.
     * Para deploy-service reutilizamos el compose actual del stack y solo reescribimos
     * las claves que el manager necesita mantener sincronizadas. */
    if matches!(site.template, crate::domain::StackTemplate::Rust) {
        let service_info = api.get_service(stack_uuid).await?;
        let current_compose = service_info
            .get("docker_compose_raw")
            .or_else(|| service_info.get("docker_compose"))
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                CoolifyError::Validation(format!(
                    "Coolify no devolvio docker_compose_raw para el stack Rust {stack_uuid}"
                ))
            })?;
        let desired_compose = rewrite_rust_service_compose(
            current_compose,
            site.repo_url
                .as_deref()
                .unwrap_or("https://github.com/1ndoryu/glory-rs.git"),
            &site.glory_branch,
            &site.dominio,
            &site.app_bin,
            &site.frontend_dir,
        )?;

        /* [04A-1] M4: Backup del compose actual antes de sobrescribir.
         * M1: Pre-flight validation del compose modificado. */
        let service_data = api.get_service(stack_uuid).await?;
        let current_compose_for_backup = service_data
            .get("docker_compose_raw")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        backup_compose_locally(&site.nombre, current_compose_for_backup)?;
        let validation = validate_compose_before_deploy(&desired_compose, "app");
        for w in &validation.warnings {
            tracing::warn!("Pre-flight warning: {}", w);
        }
        if !validation.is_ok() {
            for e in &validation.errors {
                tracing::error!("Pre-flight error: {}", e);
            }
            return Err(CoolifyError::Validation(format!(
                "Pre-flight compose validation falló: {}",
                validation.errors.join("; ")
            )));
        }

        /* [incident-2026-07-02] E19: Verificar que POSTGRES_USER/POSTGRES_DB no cambian
         * entre el compose actual y el que se va a deployear. Esto previene pérdida de datos
         * por regeneración accidental del compose (como ocurrió con glory-rest). */
        validate_postgres_creds_stable(current_compose, &desired_compose, &site.nombre)?;

        api.update_stack_compose(stack_uuid, &desired_compose)
            .await?;
        return Ok(());
    }
    let template_name = format!("{}-stack.yaml", site.template);
    let template_path = config_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("templates")
        .join(&template_name);

    if !template_path.exists() {
        return Err(CoolifyError::Template(format!(
            "Template '{}' no encontrado en {}",
            template_name,
            template_path.display()
        )));
    }

    let mut compose_vars = match site.template {
        crate::domain::StackTemplate::Rust => {
            let repo_url = site
                .repo_url
                .as_deref()
                .unwrap_or("https://github.com/1ndoryu/glory-rs.git");
            template_engine::rust_vars_full(
                &site.dominio,
                &site.glory_branch,
                repo_url,
                &site.nombre,
                &site.extra_domains,
                &site.app_bin,
                &site.frontend_dir,
            )
        }
        /* Otros templates pueden añadirse aqui en el futuro */
        _ => {
            return Err(CoolifyError::Validation(format!(
                "deploy-service no soporta el template '{}' aun. Usa deploy para WordPress.",
                site.template
            )));
        }
    };
    compose_vars.insert("STACK_UUID".to_string(), stack_uuid.to_string());
    compose_vars.insert(
        "HEALTH_PATH".to_string(),
        normalize_health_path(&site.health_check.http_path),
    );

    let compose_yaml = template_engine::render_file(&template_path, &compose_vars)?;
    api.update_stack_compose(stack_uuid, &compose_yaml).await?;
    Ok(())
}

pub(crate) fn rewrite_rust_service_compose(
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
    if compose.lines().any(|l| l.trim_start().starts_with("FRONTEND_DIR:")) {
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

pub(crate) fn replace_compose_key_value(
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

/// [21C-7] Inyecta `- pg_data:/var/lib/postgresql/data` en el servicio postgres si falta.
/// Stacks legacy pueden no tener el volumen montado → E18 bloquea el deploy.
/// Busca el bloque `postgres:` dentro de `services:`, luego su sub-bloque `volumes:`.
/// Si no existe `volumes:` en postgres, lo crea. Si existe pero falta el mount, lo agrega.
pub(crate) fn inject_postgres_data_volume(compose: &str) -> String {
    if compose.contains(":/var/lib/postgresql/data") {
        return compose.to_string();
    }
    let ends_with_newline = compose.ends_with('\n');
    let lines: Vec<&str> = compose.lines().collect();
    let mut result: Vec<String> = Vec::with_capacity(lines.len() + 2);
    let mut injected = false;

    /* Fase 1: encontrar el servicio postgres y su bloque volumes */
    let mut in_services = false;
    let mut services_indent: isize = -1;
    let mut in_postgres = false;
    let mut postgres_indent: usize = 0;
    let mut postgres_has_volumes_block = false;
    let mut postgres_volumes_indent: usize = 0;
    let _inserted_after_volumes = false;

    /* Primera pasada: detectar si existe el bloque volumes: en postgres */
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if trimmed == "services:" {
            in_services = true;
            services_indent = indent as isize;
            continue;
        }
        if in_services
            && services_indent >= 0
            && indent == (services_indent as usize + 2)
            && trimmed.ends_with(':')
            && !trimmed.contains(' ')
            && !trimmed.starts_with('-')
        {
            let svc = trimmed.trim_end_matches(':');
            in_postgres = svc == "postgres";
            postgres_indent = indent;
            if svc != "postgres" {
                postgres_has_volumes_block = false;
            }
        }
        if in_services && indent <= services_indent as usize && trimmed != "services:" {
            in_services = false;
            in_postgres = false;
        }
        if in_postgres && indent == postgres_indent + 2 && trimmed == "volumes:" {
            postgres_has_volumes_block = true;
            postgres_volumes_indent = indent;
        }
        /* Fin del bloque volumes: si encontramos algo al mismo nivel o superior */
        if postgres_has_volumes_block && indent <= postgres_volumes_indent && trimmed != "volumes:"
        {
            postgres_has_volumes_block = false;
        }
    }

    /* Segunda pasada: inyectar el mount */
    postgres_indent = 0;
    let mut in_pg_volumes = false;
    let mut pg_vol_indent: usize = 0;
    let mut last_pg_vol_line: isize = -1;

    /* Si ya tiene bloque volumes, insertar después de la última línea del bloque.
     * Si no tiene, necesitamos crear el bloque. */
    if postgres_has_volumes_block {
        /* Encontrar la última línea del bloque volumes de postgres */
        in_services = false;
        services_indent = -1;
        in_postgres = false;
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                if in_pg_volumes {
                    last_pg_vol_line = i as isize;
                }
                continue;
            }
            let indent = line.len() - line.trim_start().len();
            if trimmed == "services:" {
                in_services = true;
                services_indent = indent as isize;
                continue;
            }
            if in_services
                && services_indent >= 0
                && indent == (services_indent as usize + 2)
                && trimmed.ends_with(':')
                && !trimmed.contains(' ')
                && !trimmed.starts_with('-')
            {
                let svc = trimmed.trim_end_matches(':');
                in_postgres = svc == "postgres";
                postgres_indent = indent;
                in_pg_volumes = false;
            }
            if in_services && indent <= services_indent as usize && trimmed != "services:" {
                in_services = false;
                in_postgres = false;
                in_pg_volumes = false;
            }
            if in_postgres && indent == postgres_indent + 2 && trimmed == "volumes:" {
                in_pg_volumes = true;
                pg_vol_indent = indent;
                last_pg_vol_line = i as isize;
            }
            if in_pg_volumes && indent <= pg_vol_indent && trimmed != "volumes:" {
                in_pg_volumes = false;
            }
            if in_pg_volumes {
                last_pg_vol_line = i as isize;
            }
        }
    }

    /* Reconstruir el compose con la inyección */
    in_services = false;
    services_indent = -1;
    in_postgres = false;
    postgres_indent = 0;
    in_pg_volumes = false;
    pg_vol_indent = 0;
    let mut current_line: isize = -1;

    for line in &lines {
        current_line += 1;
        let trimmed = line.trim();

        /* Caso A: tiene bloque volumes — insertar después de la última línea */
        if postgres_has_volumes_block && !injected && current_line == last_pg_vol_line + 1 {
            let vol_indent = " ".repeat(postgres_indent + 4);
            result.push(format!("{vol_indent}- pg_data:/var/lib/postgresql/data"));
            injected = true;
        }

        /* Caso B: no tiene bloque volumes — insertar el bloque completo antes de
         * la siguiente clave al nivel de postgres (environment, depends_on, etc.) */
        if !postgres_has_volumes_block && !injected && in_postgres {
            let indent = line.len() - line.trim_start().len();
            if indent == postgres_indent + 2
                && (trimmed.starts_with("environment:")
                    || trimmed.starts_with("depends_on:")
                    || trimmed.starts_with("healthcheck:")
                    || trimmed.starts_with("restart:")
                    || trimmed.starts_with("labels:")
                    || trimmed.starts_with("image:"))
            {
                let block_indent = " ".repeat(postgres_indent + 2);
                let item_indent = " ".repeat(postgres_indent + 4);
                result.push(format!("{block_indent}volumes:"));
                result.push(format!("{item_indent}- pg_data:/var/lib/postgresql/data"));
                injected = true;
            }
        }

        result.push(line.to_string());

        /* Tracking de contexto (después de push para no duplicar) */
        if trimmed == "services:" {
            in_services = true;
            services_indent = (line.len() - line.trim_start().len()) as isize;
        }
        if in_services
            && services_indent >= 0
            && (line.len() - line.trim_start().len()) == (services_indent as usize + 2)
            && trimmed.ends_with(':')
            && !trimmed.contains(' ')
            && !trimmed.starts_with('-')
        {
            let svc = trimmed.trim_end_matches(':');
            in_postgres = svc == "postgres";
            postgres_indent = line.len() - line.trim_start().len();
            in_pg_volumes = false;
        }
        if in_services
            && (line.len() - line.trim_start().len()) <= services_indent as usize
            && trimmed != "services:"
        {
            in_services = false;
            in_postgres = false;
        }
        if in_postgres
            && (line.len() - line.trim_start().len()) == postgres_indent + 2
            && trimmed == "volumes:"
        {
            in_pg_volumes = true;
            pg_vol_indent = line.len() - line.trim_start().len();
        }
        if in_pg_volumes
            && (line.len() - line.trim_start().len()) <= pg_vol_indent
            && trimmed != "volumes:"
        {
            in_pg_volumes = false;
        }
    }

    let mut output = result.join("\n");
    if ends_with_newline {
        output.push('\n');
    }
    if !injected {
        eprintln!(
            "[WARN] inject_postgres_data_volume: no se encontró el servicio 'postgres' en el compose. Volumen no inyectado."
        );
    }
    output
}

pub(crate) fn rewrite_compose_host_rules(compose: &str, domain_host: &str) -> String {
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

pub(crate) fn normalize_domain_host(domain: &str) -> &str {
    domain
        .trim()
        .trim_end_matches('/')
        .trim_start_matches("https://")
        .trim_start_matches("http://")
}
