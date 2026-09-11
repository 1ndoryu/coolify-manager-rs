use super::postgres_inspect::{
    extract_database_url_from_compose, extract_postgres_env_from_compose,
};
use crate::error::CoolifyError;

/* [incident-2026-07-02] E19: Validar que las credenciales PostgreSQL no cambian entre
 * el compose actual (en Coolify) y el compose que se va a deployear.
 *
 * Esto previene el escenario donde Coolify regenera el compose y cambia
 * POSTGRES_USER/POSTGRES_DB (ej: glory_app/glory → rust_app/rust_db),
 * causando que el contenedor postgres cree una base de datos nueva vacía
 * y la app corra migraciones sobre ella, perdiendo todos los datos.
 *
 * Retorna Ok(()) si las credenciales son estables, Err si cambian. */
pub(crate) fn validate_postgres_creds_stable(
    current_compose: &str,
    desired_compose: &str,
    site_name: &str,
) -> std::result::Result<(), CoolifyError> {
    let current_creds = extract_postgres_env_from_compose(current_compose);
    let desired_creds = extract_postgres_env_from_compose(desired_compose);

    match (current_creds, desired_creds) {
        (Some((cur_user, cur_db)), Some((des_user, des_db))) => {
            if cur_user != des_user || cur_db != des_db {
                return Err(CoolifyError::Validation(format!(
                    "E19: Credenciales PostgreSQL cambiaron en el compose de '{}'. \
                     Actual: POSTGRES_USER={}, POSTGRES_DB={}. \
                     Nuevo: POSTGRES_USER={}, POSTGRES_DB={}. \
                     Esto causaria pérdida de datos al crear una base nueva vacía. \
                     Si el cambio es intencional, usa `deploy` con --force-postgres-drift \
                     o corrige manualmente via Coolify UI. \
                     Historico: glory-rest perdio datos el 2026-07-01 por este mecanismo.",
                    site_name, cur_user, cur_db, des_user, des_db
                )));
            }
            /* También verificar coherencia interna: POSTGRES_USER/DB vs DATABASE_URL */
            if let Some((url_user, url_db)) = extract_database_url_from_compose(desired_compose) {
                if url_user != des_user || url_db != des_db {
                    tracing::warn!(
                        "E19: DATABASE_URL user/db ({}/{}) no coincide con POSTGRES_USER/DB ({}/{}) en compose de '{}'",
                        url_user, url_db, des_user, des_db, site_name
                    );
                }
            }
        }
        (None, Some((des_user, des_db))) => {
            /* Compose actual no tiene POSTGRES_USER/DB explícitos (podría venir de template)
             * pero el nuevo sí. Esto es OK en creación inicial, pero warn. */
            tracing::info!(
                "E19: Compose actual de '{}' no tiene POSTGRES_USER/DB explicitos; \
                 nuevo compose define {}/{}. Esto es normal en primer deploy.",
                site_name,
                des_user,
                des_db
            );
        }
        (Some(_), None) => {
            tracing::warn!(
                "E19: Compose actual de '{}' tiene POSTGRES_USER/DB pero el nuevo no los define.",
                site_name
            );
        }
        (None, None) => { /* Ambos sin POSTGRES_USER/DB explicitos — OK, Coolify usa defaults */ }
    }
    Ok(())
}

/* [04A-1] M1: Pre-flight compose validation.
 * Resuelve E4 (backticks), E15 (sin diff), E16 (busybox), E17 (bind mount wrong).
 * Retorna lista de errores (bloqueantes) y warnings (no bloqueantes). */
pub(crate) struct ComposeValidation {
    pub(crate) errors: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

impl ComposeValidation {
    pub(crate) fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }
    pub(crate) fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

pub(crate) fn validate_compose_before_deploy(
    compose: &str,
    service_name: &str,
) -> ComposeValidation {
    let mut result = ComposeValidation::new();

    /* E4: Verificar backticks en Host() rules */
    for line in compose.lines() {
        let trimmed = line.trim();
        if trimmed.contains("Host(") && !trimmed.contains("Host(`") {
            result
                .errors
                .push(format!("E4: Host() rule sin backticks: '{}'", trimmed));
        }
    }

    /* E16: Verificar que imagen no es busybox en servicio target */
    let mut current_service = "";
    for line in compose.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("  ") && trimmed.ends_with(':') && !trimmed.contains(' ') {
            current_service = trimmed.trim_end_matches(':');
        }
        if current_service == service_name && trimmed.contains("image: busybox") {
            result.errors.push(format!(
                "E16: Servicio '{}' usa busybox:latest como imagen",
                service_name
            ));
        }
    }

    /* E17: Verificar que bind mount /app/uploads está en servicio correcto */
    let mut service_with_uploads: Option<String> = None;
    let mut current_svc = "";
    for line in compose.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with(' ') && !trimmed.starts_with('-') && trimmed.ends_with(':') {
            current_svc = trimmed.trim_end_matches(':');
        }
        if trimmed.contains("/app/uploads") && !trimmed.starts_with('#') {
            service_with_uploads = Some(current_svc.to_string());
        }
    }
    if let Some(svc) = &service_with_uploads {
        if svc != service_name && svc != "app" {
            result.warnings.push(format!(
                "E17: Bind mount /app/uploads en servicio '{}' (debería estar en '{}')",
                svc, service_name
            ));
        }
    }

    /* [incident-2026-07-01] E18: Verificar que PostgreSQL tiene volumen de datos montado.
     * Sin volumen de datos en /var/lib/postgresql/data, los datos se pierden al recrear
     * el contenedor. Coolify prefija los nombres de volumen con el stack UUID
     * (ej: mo4so..._pg-data), por lo que buscamos el destino `:/var/lib/postgresql/data`
     * sin exigir un nombre fijo de volumen.
     *
     * Detectamos servicios como claves directas bajo `services:` (indent = services_indent + 2).
     * Coolify usa 2 espacios por nivel; procesados pueden usar 4. Adaptamos el nivel. */
    let mut postgres_service_found = false;
    let mut postgres_has_volume = false;
    let mut in_services = false;
    let mut services_indent: isize = -1;
    let mut current_svc = "";
    let mut svc_indent: usize = 0;
    let mut in_postgres_volumes = false;
    let mut pg_volumes_indent: usize = 0;
    for line in compose.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();

        /* Detect top-level 'services:' key */
        if trimmed == "services:" {
            in_services = true;
            services_indent = indent as isize;
            continue;
        }

        /* Detect service name: direct child of services (indent = services_indent + step) */
        if in_services
            && services_indent >= 0
            && indent == (services_indent as usize + 2)
            && trimmed.ends_with(':')
            && !trimmed.contains(' ')
            && !trimmed.starts_with('-')
        {
            current_svc = trimmed.trim_end_matches(':');
            svc_indent = indent;
            in_postgres_volumes = false;
        }

        /* End of services block when we hit a sibling or parent indent */
        if in_services && indent <= services_indent as usize && trimmed != "services:" {
            in_services = false;
            current_svc = "";
        }

        if current_svc == "postgres" {
            postgres_service_found = true;
            /* Detect volumes: block inside postgres service */
            if indent == svc_indent + 2 && trimmed == "volumes:" {
                in_postgres_volumes = true;
                pg_volumes_indent = indent;
            }
            /* End of volumes sub-block */
            if in_postgres_volumes && indent <= pg_volumes_indent && trimmed != "volumes:" {
                in_postgres_volumes = false;
            }
            /* Check for any named volume mapped to /var/lib/postgresql/data */
            if in_postgres_volumes && trimmed.contains(":/var/lib/postgresql/data") {
                postgres_has_volume = true;
            }
        }
    }
    if postgres_service_found && !postgres_has_volume {
        result.errors.push(
            "E18: Servicio 'postgres' declarado pero sin volumen de datos en /var/lib/postgresql/data — datos se pierden al recrear contenedor".to_string()
        );
    }

    /* [incident-2026-07-21] E19: Verificar que traefik.docker.network=coolify existe.
     * Sin este label, Traefik no puede encontrar el contenedor en la red correcta
     * y devuelve 503 "no available server" aunque la app esté corriendo.
     * Sitios legacy (creados antes de [235A-4]) no lo tienen. */
    if compose.contains("traefik.enable=true")
        && !compose.contains("traefik.docker.network=coolify")
    {
        result.warnings.push(
            "E19: Label 'traefik.docker.network=coolify' faltante. Traefik no encontrará el contenedor → 503. inject_traefik_network_label() debe corregirlo.".to_string()
        );
    }

    result
}
