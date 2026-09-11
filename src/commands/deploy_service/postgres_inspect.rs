use regex::Regex;

/* [incident-2026-07-02] Extraer variables de entorno POSTGRES_USER y POSTGRES_DB
 * de un compose YAML. Busca en environment: del servicio postgres.
 * Soporta formato lista (- KEY=VALUE) y formato mapa (KEY: VALUE).
 * Retorna (POSTGRES_USER, POSTGRES_DB) o None si no se encuentran ambas. */
pub(crate) fn extract_postgres_env_from_compose(compose: &str) -> Option<(String, String)> {
    let mut in_postgres = false;
    let mut in_env = false;
    let mut pg_indent: usize = 0;
    let mut env_indent: usize = 0;
    let mut user = None;
    let mut db = None;

    for line in compose.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();

        /* Detect service name at services level (indent ~2 or 4) */
        if !trimmed.starts_with('-') && trimmed.ends_with(':') && !trimmed.contains(' ') {
            let svc_name = trimmed.trim_end_matches(':');
            if svc_name == "postgres" {
                in_postgres = true;
                pg_indent = indent;
            } else if in_postgres && indent <= pg_indent {
                /* Sibling service — exit postgres block */
                in_postgres = false;
                in_env = false;
            }
        }

        /* End of postgres block when hitting same or lower indent */
        if in_postgres
            && indent <= pg_indent
            && !trimmed.starts_with('-')
            && trimmed.ends_with(':')
            && trimmed != "postgres:"
        {
            /* Could be a sub-key like environment:, volumes: */
        }

        if !in_postgres {
            continue;
        }

        /* Detect environment: key */
        if indent == pg_indent + 2 && trimmed == "environment:" {
            in_env = true;
            env_indent = indent;
            continue;
        }

        /* End of environment block */
        if in_env && indent <= env_indent && trimmed != "environment:" {
            in_env = false;
            continue;
        }

        if !in_env {
            continue;
        }

        /* Parse env vars: format list (- KEY=VALUE) or map (KEY: VALUE) */
        let env_line = if let Some(rest) = trimmed.strip_prefix("- ") {
            rest.trim()
        } else {
            trimmed
        };

        /* Map format: KEY: VALUE or KEY: "VALUE" */
        if let Some((key, val)) = env_line.split_once(':') {
            let key = key.trim();
            let val = val.trim().trim_matches(|c| c == '"' || c == '\'');
            if key == "POSTGRES_USER" {
                user = Some(val.to_string());
            } else if key == "POSTGRES_DB" {
                db = Some(val.to_string());
            }
        }
        /* List format: KEY=VALUE */
        else if let Some((key, val)) = env_line.split_once('=') {
            let key = key.trim();
            let val = val.trim().trim_matches(|c| c == '"' || c == '\'');
            if key == "POSTGRES_USER" {
                user = Some(val.to_string());
            } else if key == "POSTGRES_DB" {
                db = Some(val.to_string());
            }
        }
    }

    match (user, db) {
        (Some(u), Some(d)) => Some((u, d)),
        _ => None,
    }
}

/* [incident-2026-07-02] Extraer usuario y base de datos de DATABASE_URL en compose.
 * Busca la variable DATABASE_URL en el environment del servicio app.
 * Formato: postgres://user:pass@host:port/dbname */
pub(crate) fn extract_database_url_from_compose(compose: &str) -> Option<(String, String)> {
    /* Buscar DATABASE_URL en formato lista o mapa */
    let re =
        Regex::new(r#"DATABASE_URL\s*[:=]\s*['"]?postgres(?:ql)?://([^:]+):[^@]+@[^/]+/(\w+)"#)
            .ok()?;
    for line in compose.lines() {
        let trimmed = line.trim().trim_start_matches('-').trim();
        if let Some(caps) = re.captures(trimmed) {
            let user = caps.get(1)?.as_str().to_string();
            let db = caps.get(2)?.as_str().to_string();
            return Some((user, db));
        }
    }
    None
}
