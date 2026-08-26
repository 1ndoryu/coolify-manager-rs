/*
 * Motor de templates para generar Docker Compose YAML.
 * Reemplaza placeholders {{VAR}} con valores proporcionados.
 */

use crate::error::CoolifyError;
use std::collections::HashMap;
use std::path::Path;

/// Renderiza un template reemplazando placeholders {{KEY}} con valores del mapa.
pub fn render(template: &str, vars: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        let placeholder = format!("{{{{{}}}}}", key);
        result = result.replace(&placeholder, value);
    }
    result
}

/// Carga un template desde archivo y lo renderiza.
pub fn render_file(
    template_path: &Path,
    vars: &HashMap<String, String>,
) -> std::result::Result<String, CoolifyError> {
    let template = std::fs::read_to_string(template_path).map_err(|e| {
        CoolifyError::Template(format!(
            "No se pudo leer template '{}': {e}",
            template_path.display()
        ))
    })?;
    Ok(render(&template, vars))
}

/// [268A-5] Convierte un compose a ASCII puro antes de enviarlo a Coolify.
///
/// Coolify hasta 4.0.0-beta.460 valida el compose decodificado con
/// `mb_detect_encoding($s, 'ASCII', true)`: cualquier byte >127 (acentos,
/// em-dash, BOM...) hace que la API devuelva 422 con el mensaje ENGAÑOSO
/// "docker_compose_raw should be base64 encoded" aunque el base64 sea válido.
/// Por eso el manager translitera caracteres latinos comunes a ASCII y descarta
/// el resto ANTES de base64-encodear, tanto en create_stack como en
/// update_stack_compose. Los templates se mantienen ASCII por higiene.
pub fn to_ascii_safe(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut replaced = 0usize;
    for ch in input.chars() {
        if ch.is_ascii() {
            out.push(ch);
        } else {
            let replacement: &str = match ch {
                'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => "a",
                'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' => "A",
                'é' | 'è' | 'ê' | 'ë' => "e",
                'É' | 'È' | 'Ê' | 'Ë' => "E",
                'í' | 'ì' | 'î' | 'ï' => "i",
                'Í' | 'Ì' | 'Î' | 'Ï' => "I",
                'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' => "o",
                'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' | 'Ø' => "O",
                'ú' | 'ù' | 'û' | 'ü' => "u",
                'Ú' | 'Ù' | 'Û' | 'Ü' => "U",
                'ñ' => "n",
                'Ñ' => "N",
                'ç' => "c",
                'Ç' => "C",
                'ß' => "ss",
                'æ' => "ae",
                'Æ' => "AE",
                'œ' => "oe",
                'Œ' => "OE",
                '–' | '—' | '―' => "-",
                '‘' | '’' => "'",
                '“' | '”' => "\"",
                '…' => "...",
                '¿' => "?",
                '¡' => "!",
                '€' => "EUR",
                _ => "?",
            };
            out.push_str(replacement);
            replaced += 1;
        }
    }
    if replaced > 0 {
        tracing::warn!(
            "to_ascii_safe: {replaced} caracteres no-ASCII reemplazados (Coolify beta.460 exige compose ASCII puro)"
        );
    }
    out
}

fn clean_domain(domain: &str) -> String {
    domain
        .trim()
        .trim_end_matches('/')
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_string()
}

fn domain_slug(domain_clean: &str) -> String {
    domain_clean
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn rust_extra_domain_labels(extra_domains: &[String], primary_service_slug: &str) -> String {
    extra_domains
        .iter()
        .map(|domain| clean_domain(domain))
        .filter(|domain| !domain.is_empty())
        .map(|domain| {
            let slug = domain_slug(&domain);
            format!(
                r#"            - "traefik.http.routers.{slug}-https.rule=Host(`{domain}`)"
            - "traefik.http.routers.{slug}-https.entryPoints=https"
            - "traefik.http.routers.{slug}-https.tls=true"
            - "traefik.http.routers.{slug}-https.tls.certresolver=letsencrypt"
            - "traefik.http.routers.{slug}-https.service={primary_service_slug}-svc"
            - "traefik.http.routers.{slug}-http.rule=Host(`{domain}`)"
            - "traefik.http.routers.{slug}-http.entryPoints=http"
            - "traefik.http.routers.{slug}-http.middlewares={slug}-redirect"
            - "traefik.http.middlewares.{slug}-redirect.redirectscheme.scheme=https""#
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(clippy::too_many_arguments)]
pub fn wordpress_vars(
    domain: &str,
    db_password: &str,
    root_password: &str,
    theme_repo: &str,
    library_repo: &str,
    glory_branch: &str,
    library_branch: &str,
    theme_name: &str,
) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    vars.insert("DOMAIN".to_string(), domain.to_string());
    vars.insert("DB_PASSWORD".to_string(), db_password.to_string());
    vars.insert("ROOT_PASSWORD".to_string(), root_password.to_string());
    /* [F5] Variables para labels Traefik explícitos */
    let domain_clean = domain
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    vars.insert("DOMAIN_CLEAN".to_string(), domain_clean.to_string());
    let domain_slug = domain_clean.replace('.', "-");
    vars.insert("DOMAIN_SLUG".to_string(), domain_slug);
    vars.insert("GLORY_THEME_REPO".to_string(), theme_repo.to_string());
    vars.insert("GLORY_LIBRARY_REPO".to_string(), library_repo.to_string());
    vars.insert("GLORY_BRANCH".to_string(), glory_branch.to_string());
    vars.insert(
        "GLORY_LIBRARY_BRANCH".to_string(),
        library_branch.to_string(),
    );
    vars.insert("GLORY_THEME_NAME".to_string(), theme_name.to_string());
    vars
}

#[allow(clippy::too_many_arguments)]
pub fn kamples_vars(
    domain: &str,
    db_password: &str,
    root_password: &str,
    pg_password: &str,
    glory_branch: &str,
    theme_repo: &str,
    library_repo: &str,
    library_branch: &str,
    theme_name: &str,
) -> HashMap<String, String> {
    let mut vars = wordpress_vars(
        domain,
        db_password,
        root_password,
        theme_repo,
        library_repo,
        glory_branch,
        library_branch,
        theme_name,
    );
    vars.insert("PG_PASSWORD".to_string(), pg_password.to_string());

    /* WebSocket service — secrets y dominios derivados */
    let ws_internal_secret = generate_password(32);
    let ws_ticket_secret = generate_password(32);
    vars.insert("WS_INTERNAL_SECRET".to_string(), ws_internal_secret);
    vars.insert("WS_TICKET_SECRET".to_string(), ws_ticket_secret);

    let domain_clean = domain
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    vars.insert(
        "WS_DOMAIN".to_string(),
        format!("https://ws.{domain_clean}"),
    );
    vars.insert(
        "WS_PUBLIC_URL".to_string(),
        format!("wss://ws.{domain_clean}"),
    );
    vars.insert("GLORY_BRANCH".to_string(), glory_branch.to_string());

    vars
}

/// Genera las variables para un stack de Minecraft.
pub fn minecraft_vars(server_name: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    vars.insert("SERVER_NAME".to_string(), server_name.to_string());
    vars
}

/// Genera las variables para un stack de Rust (Axum/Actix + PostgreSQL).
/// Contraseñas se delegan a Coolify ($SERVICE_PASSWORD_*).
pub fn rust_vars(
    domain: &str,
    glory_branch: &str,
    repo_url: &str,
    site_name: &str,
) -> HashMap<String, String> {
    rust_vars_full(domain, glory_branch, repo_url, site_name, &[], "glory-backend", "frontend")
}

/// Genera las variables para un stack Rust con dominios adicionales.
pub fn rust_vars_with_extra_domains(
    domain: &str,
    glory_branch: &str,
    repo_url: &str,
    site_name: &str,
    extra_domains: &[String],
) -> HashMap<String, String> {
    rust_vars_full(domain, glory_branch, repo_url, site_name, extra_domains, "glory-backend", "frontend")
}

/// [268A-4] Variante completa: permite fijar el binario Rust y el directorio del
/// frontend (proyectos no-glory como ong-agape). Retrocompatible por defaults.
pub fn rust_vars_full(
    domain: &str,
    glory_branch: &str,
    repo_url: &str,
    site_name: &str,
    extra_domains: &[String],
    app_bin: &str,
    frontend_dir: &str,
) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    vars.insert("DOMAIN".to_string(), domain.to_string());
    /* DOMAIN_CLEAN: sin protocolo, para SERVICE_FQDN */
    let domain_clean = clean_domain(domain);
    vars.insert("DOMAIN_CLEAN".to_string(), domain_clean.to_string());
    /* DOMAIN_SLUG: sin protocolo, puntos reemplazados por guiones (para Traefik labels) */
    let primary_domain_slug = domain_slug(&domain_clean);
    vars.insert("DOMAIN_SLUG".to_string(), primary_domain_slug.clone());
    vars.insert(
        "EXTRA_DOMAIN_LABELS".to_string(),
        rust_extra_domain_labels(extra_domains, &primary_domain_slug),
    );
    vars.insert("GLORY_BRANCH".to_string(), glory_branch.to_string());
    vars.insert("REPO_URL".to_string(), repo_url.to_string());
    /* Nombre del binario Rust principal (Cargo package name) */
    vars.insert("APP_BIN".to_string(), app_bin.to_string());
    /* [268A-4] Directorio del frontend dentro del repo (proyectos no-glory) */
    vars.insert("FRONTEND_DIR".to_string(), frontend_dir.to_string());
    /* [114A-6] Nombre del sitio para bind mount persistente de uploads */
    vars.insert("SITE_NAME".to_string(), site_name.to_string());
    /* [25A-DB-AUTH] Placeholder: se reemplaza en new_site::execute() con el UUID real del stack
     * tras create_stack(), para que container_name y DATABASE_URL usen postgres-{uuid} */
    vars.insert(
        "STACK_UUID".to_string(),
        "STACK_UUID_PLACEHOLDER".to_string(),
    );
    /* [268A-5] HEALTH_PATH: el template rust-stack.yaml usa {{HEALTH_PATH}} en el
     * healthcheck del contenedor. Antes este placeholder quedaba literal en el render
     * (el manager nunca lo proveía) y el healthcheck ejecutaba una URL rota. */
    vars.insert("HEALTH_PATH".to_string(), "/api/health".to_string());
    vars
}

/// Genera un password aleatorio seguro.
pub fn generate_password(length: usize) -> String {
    use rand::Rng;
    let charset = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..charset.len());
            charset[idx] as char
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_simple() {
        let template = "Hello {{NAME}}, welcome to {{PLACE}}!";
        let mut vars = HashMap::new();
        vars.insert("NAME".to_string(), "World".to_string());
        vars.insert("PLACE".to_string(), "Rust".to_string());
        let result = render(template, &vars);
        assert_eq!(result, "Hello World, welcome to Rust!");
    }

    #[test]
    fn test_render_docker_compose() {
        let template = r#"services:
    wordpress:
        environment:
            WORDPRESS_DB_PASSWORD: {{DB_PASSWORD}}
            SERVICE_FQDN_WORDPRESS: {{DOMAIN}}"#;

        let vars = wordpress_vars(
            "https://blog.com",
            "secret123",
            "rootpass",
            "",
            "",
            "main",
            "main",
            "glorytemplate",
        );
        let result = render(template, &vars);
        assert!(result.contains("secret123"));
        assert!(result.contains("https://blog.com"));
    }

    #[test]
    fn test_render_no_vars_unchanged() {
        let template = "no placeholders here";
        let vars = HashMap::new();
        assert_eq!(render(template, &vars), template);
    }

    #[test]
    fn test_render_missing_var_left_unchanged() {
        let template = "value: {{MISSING}}";
        let vars = HashMap::new();
        assert_eq!(render(template, &vars), "value: {{MISSING}}");
    }

    #[test]
    fn test_rust_vars_include_extra_domain_labels() {
        let vars = rust_vars_with_extra_domains(
            "https://example.com",
            "main",
            "repo",
            "studio",
            &["https://portal.example.com".to_string()],
        );
        let labels = vars.get("EXTRA_DOMAIN_LABELS").unwrap();
        assert!(labels.contains("Host(`portal.example.com`)"));
        assert!(labels.contains("portal-example-com-https.service=example-com-svc"));
    }

    #[test]
    fn test_rust_extra_domain_labels_keep_yaml_list_indentation() {
        let vars = rust_vars_with_extra_domains(
            "https://example.com",
            "main",
            "repo",
            "studio",
            &["https://portal.example.com".to_string()],
        );
        let template = r#"labels:
            - "traefik.enable=true"
{{EXTRA_DOMAIN_LABELS}}"#;
        let rendered = render(template, &vars);

        assert!(rendered
            .contains("\n            - \"traefik.http.routers.portal-example-com-https.rule"));
        assert!(
            !rendered.contains("\n                    - \"traefik.http.routers.portal-example-com")
        );
    }

    /* [268A-5] Regresión: el compose enviado a Coolify debe ser ASCII puro.
     * Coolify beta.460 valida con mb_detect_encoding($s, 'ASCII', true) y
     * devuelve 422 "should be base64 encoded" (engañoso) ante bytes >127. */
    #[test]
    fn test_to_ascii_safe_strips_accents_and_non_ascii() {
        let input = "metricas de infra: métricas, únicas — Ágape ñandú";
        let out = to_ascii_safe(input);
        assert!(out.is_ascii(), "salida debe ser ASCII pura: {out}");
        assert_eq!(out, "metricas de infra: metricas, unicas - Agape nandu");
    }

    #[test]
    fn test_to_ascii_safe_keeps_ascii_untouched() {
        let input = "services:\n  app:\n    image: nginx:alpine\n";
        assert_eq!(to_ascii_safe(input), input);
    }

    #[test]
    fn test_to_ascii_safe_handles_multichar_replacements() {
        let out = to_ascii_safe("café — Straße ¿hola? …");
        assert!(out.is_ascii());
        assert_eq!(out, "cafe - Strasse ?hola? ...");
    }

    /* [268A-5] El template rust-stack.yaml usa {{HEALTH_PATH}} en el healthcheck;
     * antes quedaba literal porque rust_vars_full no lo proveía. */
    #[test]
    fn test_rust_vars_full_provides_health_path() {
        let vars = rust_vars_full(
            "https://example.com",
            "main",
            "repo",
            "studio",
            &[],
            "glory-backend",
            "frontend",
        );
        assert_eq!(vars.get("HEALTH_PATH").unwrap(), "/api/health");
    }

    #[test]
    fn test_generate_password_length() {
        let pass = generate_password(32);
        assert_eq!(pass.len(), 32);
    }

    #[test]
    fn test_generate_password_unique() {
        let p1 = generate_password(32);
        let p2 = generate_password(32);
        assert_ne!(p1, p2);
    }

    #[test]
    fn test_wordpress_vars_keys() {
        let vars = wordpress_vars(
            "d",
            "p",
            "r",
            "repo",
            "lib",
            "main",
            "main",
            "glorytemplate",
        );
        assert!(vars.contains_key("DOMAIN"));
        assert!(vars.contains_key("DB_PASSWORD"));
        assert!(vars.contains_key("ROOT_PASSWORD"));
        assert!(vars.contains_key("GLORY_THEME_REPO"));
        assert!(vars.contains_key("GLORY_LIBRARY_REPO"));
        assert!(vars.contains_key("GLORY_THEME_NAME"));
    }

    #[test]
    fn test_kamples_vars_includes_pg() {
        let vars = kamples_vars(
            "https://kamples.com",
            "p",
            "r",
            "pg",
            "main-kamples",
            "repo",
            "lib",
            "main",
            "glorytemplate",
        );
        assert!(vars.contains_key("PG_PASSWORD"));
        assert!(vars.contains_key("DOMAIN"));
        assert!(vars.contains_key("WS_INTERNAL_SECRET"));
        assert!(vars.contains_key("WS_TICKET_SECRET"));
        assert_eq!(vars.get("WS_DOMAIN").unwrap(), "https://ws.kamples.com");
        assert_eq!(vars.get("WS_PUBLIC_URL").unwrap(), "wss://ws.kamples.com");
        assert_eq!(vars.get("GLORY_BRANCH").unwrap(), "main-kamples");
    }

    #[test]
    fn test_minecraft_vars() {
        let vars = minecraft_vars("survival");
        assert_eq!(vars.get("SERVER_NAME").unwrap(), "survival");
    }

    /* [04A-1] M6: Test que verifica que TODAS las reglas Host() generadas
     * por rust_extra_domain_labels tienen backticks. Previene regresión de E4
     * (backticks faltantes en Traefik → 404 silencioso).
     * Si este test falla, Traefik rechazará las reglas y el dominio no resolverá. */
    #[test]
    fn test_extra_domain_labels_have_backticks() {
        let vars = rust_vars_with_extra_domains(
            "https://example.com",
            "main",
            "repo",
            "studio",
            &[
                "https://portal.example.com".to_string(),
                "https://admin.example.com".to_string(),
            ],
        );
        let labels = vars.get("EXTRA_DOMAIN_LABELS").unwrap();
        /* Cada Host() debe tener backticks, nunca comillas dobles */
        let host_count = labels.matches("Host(").count();
        let backtick_count = labels.matches("Host(`").count();
        assert!(
            host_count > 0,
            "No se generaron reglas Host() en EXTRA_DOMAIN_LABELS"
        );
        assert_eq!(
            host_count, backtick_count,
            "No todas las reglas Host() tienen backticks (E4 regresión):\n{}",
            labels
        );
        assert!(
            !labels.contains("Host(\""),
            "EXTRA_DOMAIN_LABELS usa comillas dobles en vez de backticks (E4 regresión):\n{}",
            labels
        );
    }

    /* [04A-1] M6: Test que DOMAIN_CLEAN nunca incluye protocolo */
    #[test]
    fn test_domain_clean_strips_protocol() {
        let cases = vec![
            ("https://example.com", "example.com"),
            ("http://sub.domain.co.uk", "sub.domain.co.uk"),
            ("https://nakomi.studio/", "nakomi.studio"),
        ];
        for (input, expected) in cases {
            let vars = rust_vars_with_extra_domains(input, "main", "repo", "test", &[]);
            assert_eq!(
                vars.get("DOMAIN_CLEAN").unwrap(),
                expected,
                "DOMAIN_CLEAN no limpió correctamente: '{}'",
                input
            );
        }
    }
}
