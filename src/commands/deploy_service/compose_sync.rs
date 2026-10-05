use super::compose_backup::backup_compose_locally;
use super::compose_validation::{validate_compose_before_deploy, validate_postgres_creds_stable};
use super::env_building::normalize_health_path;
use crate::config::CoolifyConfig;
use crate::domain::SiteConfig;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::template_engine;
use crate::infra::validation;
use std::path::Path;

/* Renderiza el template y lo envia a Coolify via API PATCH */
pub(crate) async fn sync_compose(
    config_path: &Path,
    site: &SiteConfig,
    stack_uuid: &str,
    coolify_config: &CoolifyConfig,
) -> std::result::Result<(), CoolifyError> {
    let api = CoolifyApiClient::new(coolify_config)?;

    /* [309A-1/F2] Sitios con imageRef usan el template por imagen ANTES de la
     * rama Rust: el early-return de [265A-6] dejaba inalcanzable el brazo
     * imageRef del match de abajo (brazo muerto) y el rewrite Rust falla en
     * composes por imagen (no tienen claves REPO_URL/BRANCH/APP_BIN). */
    if site.image_ref.is_some()
        && matches!(
            site.template,
            crate::domain::StackTemplate::Rust | crate::domain::StackTemplate::Kamples
        )
    {
        if matches!(site.template, crate::domain::StackTemplate::Kamples) {
            return sync_compose_kamples_image(&api, site, stack_uuid).await;
        }
        return sync_compose_image(&api, config_path, site, stack_uuid).await;
    }
    /* [265A-6] Coolify acepta el compose Rust canónico del servicio (dockerfile + args),
     * pero rechaza en PATCH el template grande de creación con dockerfile_inline.
     * Para deploy-service reutilizamos el compose actual del stack y solo reescribimos
     * las claves que el manager necesita mantener sincronizadas. */
    if matches!(site.template, crate::domain::StackTemplate::Rust) {
        return sync_compose_rust(&api, site, stack_uuid).await;
    }
    let (template_name, mut compose_vars) = match site.template {
        crate::domain::StackTemplate::Rust => {
            let repo_url = site
                .repo_url
                .as_deref()
                .unwrap_or("https://github.com/1ndoryu/glory-rs.git");
            let vars = template_engine::rust_vars_full(
                &site.dominio,
                &site.glory_branch,
                repo_url,
                &site.nombre,
                &site.extra_domains,
                &site.app_bin,
                &site.frontend_dir,
            );
            (format!("{}-stack.yaml", site.template), vars)
        }
        /* Otros templates pueden añadirse aqui en el futuro */
        _ => {
            return Err(CoolifyError::Validation(format!(
                "deploy-service no soporta el template '{}' aun. Usa deploy para WordPress.",
                site.template
            )));
        }
    };
    /* [119A-5] canonicalize: template_name viene del enum StackTemplate o literal fijo. */
    let template_path = resolve_template_path(config_path, &template_name)?;

    compose_vars.insert("STACK_UUID".to_string(), stack_uuid.to_string());
    compose_vars.insert(
        "HEALTH_PATH".to_string(),
        normalize_health_path(&site.health_check.http_path),
    );

    let compose_yaml = template_engine::render_file(&template_path, &compose_vars)?;
    api.update_stack_compose(stack_uuid, &compose_yaml).await?;
    Ok(())
}

/* Resuelve un template del manager con canonicalize fail-closed. */
fn resolve_template_path(
    config_path: &Path,
    template_name: &str,
) -> std::result::Result<std::path::PathBuf, CoolifyError> {
    validation::validar_segmento_ruta(template_name, "template")?;
    let templates_base = config_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("templates");
    let template_path =
        validation::join_segmento_seguro(&templates_base, template_name, "template")?;

    if !template_path.exists() {
        return Err(CoolifyError::Template(format!(
            "Template '{template_name}' no encontrado en {}",
            template_path.display()
        )));
    }
    Ok(template_path)
}

/* [309A-1/F2] Rama imagen de sync_compose: renderiza rust-image-stack.yaml
 * (`image: {{IMAGE_REF}}`, sin bloque build) y verifica que Coolify lo
 * persiste (su worker async regenera el compose on-disk; si lo reescribe
 * sin la imagen, el swap posterior usaría la imagen vieja). */
async fn sync_compose_image(
    api: &CoolifyApiClient,
    config_path: &Path,
    site: &SiteConfig,
    stack_uuid: &str,
) -> std::result::Result<(), CoolifyError> {
    let image_ref = site.image_ref.as_deref().unwrap_or_default();
    validation::validate_image_ref(image_ref)?;
    let repo_url = site
        .repo_url
        .as_deref()
        .unwrap_or("https://github.com/1ndoryu/glory-rs.git");
    let mut compose_vars = template_engine::with_image_ref(
        template_engine::rust_vars_full(
            &site.dominio,
            &site.glory_branch,
            repo_url,
            &site.nombre,
            &site.extra_domains,
            &site.app_bin,
            &site.frontend_dir,
        ),
        image_ref,
    );
    compose_vars.insert("STACK_UUID".to_string(), stack_uuid.to_string());
    compose_vars.insert(
        "HEALTH_PATH".to_string(),
        normalize_health_path(&site.health_check.http_path),
    );

    let template_path = resolve_template_path(config_path, "rust-image-stack.yaml")?;
    let compose_yaml = template_engine::render_file(&template_path, &compose_vars)?;
    api.update_stack_compose(stack_uuid, &compose_yaml).await?;

    let service = api.get_service(stack_uuid).await?;
    let persisted = service
        .get("docker_compose_raw")
        .or_else(|| service.get("docker_compose"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if !persisted.contains(image_ref) {
        return Err(CoolifyError::Validation(format!(
            "Coolify no persistió la imagen '{image_ref}' en el compose del stack {stack_uuid} \
             (posible regeneración async). No se continúa: reintenta el deploy."
        )));
    }
    Ok(())
}

/* [01AA-3/F2x] Rama kamples-imagen de sync_compose: a diferencia de Rust,
 * Kamples no tiene template canónico renderizable (passwords y volúmenes
 * vivos difieren por sitio); se reutiliza el compose actual del stack y solo
 * se reescribe la línea `image:` del servicio wordpress, con verificación
 * de persistencia igual que la rama Rust. */
async fn sync_compose_kamples_image(
    api: &CoolifyApiClient,
    site: &SiteConfig,
    stack_uuid: &str,
) -> std::result::Result<(), CoolifyError> {
    let image_ref = site.image_ref.as_deref().unwrap_or_default();
    validation::validate_image_ref(image_ref)?;
    let service = api.get_service(stack_uuid).await?;
    let actual = service
        .get("docker_compose_raw")
        .or_else(|| service.get("docker_compose"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if actual.trim().is_empty() {
        return Err(CoolifyError::Validation(format!(
            "Coolify no devolvió compose actual para el stack {stack_uuid}: \
             no se puede actualizar la imagen sin compose base."
        )));
    }
    let compose_yaml = reemplazar_imagen_wordpress(actual, image_ref)?;
    api.update_stack_compose(stack_uuid, &compose_yaml).await?;

    let service = api.get_service(stack_uuid).await?;
    let persisted = service
        .get("docker_compose_raw")
        .or_else(|| service.get("docker_compose"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if !persisted.contains(image_ref) {
        return Err(CoolifyError::Validation(format!(
            "Coolify no persistió la imagen '{image_ref}' en el compose del stack {stack_uuid} \
             (posible regeneración async). No se continúa: reintenta el deploy."
        )));
    }
    Ok(())
}

/* [01AA-3/F2x] Reescritura quirúrgica de la imagen del servicio wordpress en
 * un compose Kamples ya persistido: solo cambia la línea `image:` dentro del
 * bloque `wordpress:`, conserva el resto byte a byte (passwords, volúmenes,
 * resto de servicios). Fail-closed: sin bloque wordpress, sin clave image,
 * o más de una sustitución → error, nunca un compose a medias. */
fn reemplazar_imagen_wordpress(
    compose_actual: &str,
    image_ref: &str,
) -> std::result::Result<String, CoolifyError> {
    let indent_servicio = compose_actual.lines().find_map(|line| {
        if line.trim() == "wordpress:" {
            Some(line.len() - line.trim_start().len())
        } else {
            None
        }
    });
    let indent_servicio = indent_servicio.ok_or_else(|| {
        CoolifyError::Validation(
            "El compose actual no tiene servicio 'wordpress:': no se puede actualizar la imagen."
                .to_string(),
        )
    })?;

    let mut salida: Vec<String> = Vec::new();
    let mut dentro = false;
    let mut sustituidas = 0u32;
    for line in compose_actual.lines() {
        if !dentro {
            if line.trim() == "wordpress:"
                && line.len() - line.trim_start().len() == indent_servicio
            {
                dentro = true;
            }
            salida.push(line.to_string());
            continue;
        }
        let recorte = line.trim();
        let indent = line.len() - line.trim_start().len();
        if !recorte.is_empty() && indent <= indent_servicio {
            dentro = false;
            salida.push(line.to_string());
            continue;
        }
        if let Some(valor_crudo) = recorte.strip_prefix("image:") {
            if sustituidas >= 1 {
                return Err(CoolifyError::Validation(
                    "El servicio 'wordpress:' tiene múltiples claves 'image:': \
                     reescritura ambigua, no se continúa."
                        .to_string(),
                ));
            }
            let valor_viejo = valor_crudo.trim();
            let valor_nuevo = if valor_viejo.starts_with('\'')
                && valor_viejo.ends_with('\'')
                && valor_viejo.len() >= 2
            {
                format!("'{image_ref}'")
            } else if valor_viejo.starts_with('"')
                && valor_viejo.ends_with('"')
                && valor_viejo.len() >= 2
            {
                format!("\"{image_ref}\"")
            } else {
                image_ref.to_string()
            };
            let prefijo = &line[..line.len() - line.trim_start().len()];
            salida.push(format!("{prefijo}image: {valor_nuevo}"));
            sustituidas += 1;
            continue;
        }
        salida.push(line.to_string());
    }

    if sustituidas == 0 {
        return Err(CoolifyError::Validation(
            "El servicio 'wordpress:' no tiene clave 'image:': no se puede actualizar la imagen."
                .to_string(),
        ));
    }
    let mut updated = salida.join("\n");
    if compose_actual.ends_with('\n') {
        updated.push('\n');
    }
    Ok(updated)
}

/* [01AA-3/F2x] NOTA 2026-10-01: existió `sync_kamples_on_disk` (editar el yml
 * on-disk por SSH + `up -d` manual). Se eliminó porque el reconciliador de
 * Coolify regenera raw+disco desde el atributo `image` en minutos y revierte
 * ese cambio; el camino durable es el deploy oficial (`POST /api/v1/deploy`)
 * desde `deploy_service/mod.rs`. La reescritura quirúrgica y testeada vive
 * en `reemplazar_imagen_wordpress`, usada por `sync_compose_kamples_image`. */

/* Rama Rust de sync_compose: reutiliza el compose actual del stack y solo
 * reescribe las claves que el manager necesita mantener sincronizadas. */
async fn sync_compose_rust(
    api: &CoolifyApiClient,
    site: &SiteConfig,
    stack_uuid: &str,
) -> std::result::Result<(), CoolifyError> {
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
/* [119A-2] Pasada 1 de inject_postgres_data_volume: detectar si el servicio
 * postgres ya tiene bloque `volumes:`. */
fn detectar_bloque_volumes_postgres(lineas: &[&str]) -> bool {
    let mut en_servicios = false;
    let mut indent_servicios: isize = -1;
    let mut en_postgres = false;
    let mut indent_postgres: usize = 0;
    let mut tiene_bloque = false;
    let mut indent_volumes: usize = 0;

    for linea in lineas {
        let recortada = linea.trim();
        if recortada.is_empty() || recortada.starts_with('#') {
            continue;
        }
        let indent = linea.len() - linea.trim_start().len();
        if recortada == "services:" {
            en_servicios = true;
            indent_servicios = indent as isize;
            continue;
        }
        if en_servicios
            && indent_servicios >= 0
            && indent == (indent_servicios as usize + 2)
            && recortada.ends_with(':')
            && !recortada.contains(' ')
            && !recortada.starts_with('-')
        {
            let svc = recortada.trim_end_matches(':');
            en_postgres = svc == "postgres";
            indent_postgres = indent;
            if svc != "postgres" {
                tiene_bloque = false;
            }
        }
        if en_servicios && indent <= indent_servicios as usize && recortada != "services:" {
            en_servicios = false;
            en_postgres = false;
        }
        if en_postgres && indent == indent_postgres + 2 && recortada == "volumes:" {
            tiene_bloque = true;
            indent_volumes = indent;
        }
        /* Fin del bloque volumes: si encontramos algo al mismo nivel o superior */
        if tiene_bloque && indent <= indent_volumes && recortada != "volumes:" {
            tiene_bloque = false;
        }
    }
    tiene_bloque
}

/* [119A-2] Pasada 2: última línea del bloque volumes de postgres (índice
 * para insertar después). Solo se llama si ya existe el bloque. */
fn ultima_linea_volumes_postgres(lineas: &[&str]) -> isize {
    let mut en_servicios = false;
    let mut indent_servicios: isize = -1;
    let mut en_postgres = false;
    let mut indent_postgres: usize = 0;
    let mut en_volumes = false;
    let mut indent_vol: usize = 0;
    let mut ultima: isize = -1;

    /* Encontrar la última línea del bloque volumes de postgres */
    for (i, linea) in lineas.iter().enumerate() {
        let recortada = linea.trim();
        if recortada.is_empty() || recortada.starts_with('#') {
            if en_volumes {
                ultima = i as isize;
            }
            continue;
        }
        let indent = linea.len() - linea.trim_start().len();
        if recortada == "services:" {
            en_servicios = true;
            indent_servicios = indent as isize;
            continue;
        }
        if en_servicios
            && indent_servicios >= 0
            && indent == (indent_servicios as usize + 2)
            && recortada.ends_with(':')
            && !recortada.contains(' ')
            && !recortada.starts_with('-')
        {
            let svc = recortada.trim_end_matches(':');
            en_postgres = svc == "postgres";
            indent_postgres = indent;
            en_volumes = false;
        }
        if en_servicios && indent <= indent_servicios as usize && recortada != "services:" {
            en_servicios = false;
            en_postgres = false;
            en_volumes = false;
        }
        if en_postgres && indent == indent_postgres + 2 && recortada == "volumes:" {
            en_volumes = true;
            indent_vol = indent;
            ultima = i as isize;
        }
        if en_volumes && indent <= indent_vol && recortada != "volumes:" {
            en_volumes = false;
        }
        if en_volumes {
            ultima = i as isize;
        }
    }
    ultima
}

/* [119A-2] Pasada 3: reconstruir el compose inyectando el mount pg_data.
 * Caso A (tiene bloque): inserta después de la última línea del bloque.
 * Caso B (sin bloque): crea el bloque antes de la siguiente clave de postgres. */
fn reconstruir_con_volumen_postgres(
    lineas: &[&str],
    tiene_bloque: bool,
    ultima_volumen: isize,
    termina_con_salto: bool,
) -> String {
    let mut resultado: Vec<String> = Vec::with_capacity(lineas.len() + 2);
    let mut inyectado = false;
    let mut en_servicios = false;
    let mut indent_servicios: isize = -1;
    let mut en_postgres = false;
    let mut indent_postgres: usize = 0;
    let mut en_volumes = false;
    let mut indent_vol: usize = 0;
    let mut actual: isize = -1;

    for linea in lineas {
        actual += 1;
        let recortada = linea.trim();

        /* Caso A: tiene bloque volumes — insertar después de la última línea */
        if tiene_bloque && !inyectado && actual == ultima_volumen + 1 {
            let vol_indent = " ".repeat(indent_postgres + 4);
            resultado.push(format!("{vol_indent}- pg_data:/var/lib/postgresql/data"));
            inyectado = true;
        }

        /* Caso B: no tiene bloque volumes — insertar el bloque completo antes de
         * la siguiente clave al nivel de postgres (environment, depends_on, etc.) */
        if !tiene_bloque && !inyectado && en_postgres {
            let indent = linea.len() - linea.trim_start().len();
            if indent == indent_postgres + 2
                && (recortada.starts_with("environment:")
                    || recortada.starts_with("depends_on:")
                    || recortada.starts_with("healthcheck:")
                    || recortada.starts_with("restart:")
                    || recortada.starts_with("labels:")
                    || recortada.starts_with("image:"))
            {
                let block_indent = " ".repeat(indent_postgres + 2);
                let item_indent = " ".repeat(indent_postgres + 4);
                resultado.push(format!("{block_indent}volumes:"));
                resultado.push(format!("{item_indent}- pg_data:/var/lib/postgresql/data"));
                inyectado = true;
            }
        }

        resultado.push(linea.to_string());

        /* Tracking de contexto (después de push para no duplicar) */
        if recortada == "services:" {
            en_servicios = true;
            indent_servicios = (linea.len() - linea.trim_start().len()) as isize;
        }
        if en_servicios
            && indent_servicios >= 0
            && (linea.len() - linea.trim_start().len()) == (indent_servicios as usize + 2)
            && recortada.ends_with(':')
            && !recortada.contains(' ')
            && !recortada.starts_with('-')
        {
            let svc = recortada.trim_end_matches(':');
            en_postgres = svc == "postgres";
            indent_postgres = linea.len() - linea.trim_start().len();
            en_volumes = false;
        }
        if en_servicios
            && (linea.len() - linea.trim_start().len()) <= indent_servicios as usize
            && recortada != "services:"
        {
            en_servicios = false;
            en_postgres = false;
        }
        if en_postgres
            && (linea.len() - linea.trim_start().len()) == indent_postgres + 2
            && recortada == "volumes:"
        {
            en_volumes = true;
            indent_vol = linea.len() - linea.trim_start().len();
        }
        if en_volumes
            && (linea.len() - linea.trim_start().len()) <= indent_vol
            && recortada != "volumes:"
        {
            en_volumes = false;
        }
    }

    let mut salida = resultado.join("\n");
    if termina_con_salto {
        salida.push('\n');
    }
    if !inyectado {
        eprintln!(
            "[WARN] inject_postgres_data_volume: no se encontró el servicio 'postgres' en el compose. Volumen no inyectado."
        );
    }
    salida
}

pub(crate) fn inject_postgres_data_volume(compose: &str) -> String {
    if compose.contains(":/var/lib/postgresql/data") {
        return compose.to_string();
    }
    let ends_with_newline = compose.ends_with('\n');
    let lines: Vec<&str> = compose.lines().collect();
    /* Si ya tiene bloque volumes, insertar despues de la ultima linea del bloque.
     * Si no tiene, necesitamos crear el bloque. */
    let tiene_bloque = detectar_bloque_volumes_postgres(&lines);
    let ultima = if tiene_bloque {
        ultima_linea_volumes_postgres(&lines)
    } else {
        -1
    };
    reconstruir_con_volumen_postgres(&lines, tiene_bloque, ultima, ends_with_newline)
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

#[cfg(test)]
mod kamples_image_tests {
    use super::reemplazar_imagen_wordpress;

    const FIXTURE: &str = "services:\n    wordpress:\n        image: 'cm-local/cm-test-01aa3:20261001-142903'\n        container_name: wordpress-test\n        environment:\n            WORDPRESS_DB_PASSWORD: secreta123\n            MYSQL_PASSWORD: secreta123\n    mariadb:\n        image: mariadb:10.11\n        environment:\n            MARIADB_ROOT_PASSWORD: root456\n    postgres:\n        image: pgvector/pgvector:pg17\nvolumes:\n    db-data: {}\n    pg-data: {}\n";

    #[test]
    fn cambia_solo_imagen_wordpress() {
        let out = reemplazar_imagen_wordpress(FIXTURE, "cm-local/cm-test-01aa3:20261001-150000")
            .expect("rewrite válido");
        assert!(out.contains("image: 'cm-local/cm-test-01aa3:20261001-150000'"));
        assert!(!out.contains("20261001-142903"));
        assert!(out.contains("image: mariadb:10.11"));
        assert!(out.contains("image: pgvector/pgvector:pg17"));
        assert!(out.contains("WORDPRESS_DB_PASSWORD: secreta123"));
        assert!(out.contains("MARIADB_ROOT_PASSWORD: root456"));
    }

    #[test]
    fn conserva_valor_sin_comillas_sin_comillas() {
        let base = FIXTURE.replace(
            "image: 'cm-local/cm-test-01aa3:20261001-142903'",
            "image: wordpress:php8.2-apache",
        );
        let out = reemplazar_imagen_wordpress(&base, "cm-local/x:tag").expect("rewrite válido");
        assert!(out.contains("image: cm-local/x:tag"));
        assert!(!out.contains("wordpress:php8.2-apache"));
    }

    #[test]
    fn falla_sin_servicio_wordpress() {
        let base = FIXTURE.replace("    wordpress:\n", "    app:\n");
        assert!(reemplazar_imagen_wordpress(&base, "cm-local/x:tag").is_err());
    }

    #[test]
    fn falla_si_wordpress_sin_image() {
        let base = FIXTURE.replace(
            "        image: 'cm-local/cm-test-01aa3:20261001-142903'\n",
            "",
        );
        assert!(reemplazar_imagen_wordpress(&base, "cm-local/x:tag").is_err());
    }
}
