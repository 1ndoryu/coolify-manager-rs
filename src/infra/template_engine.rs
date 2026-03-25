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

/// Genera las variables para un stack de WordPress.
pub fn wordpress_vars(
    domain: &str,
    db_password: &str,
    root_password: &str,
) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    vars.insert("DOMAIN".to_string(), domain.to_string());
    vars.insert("DB_PASSWORD".to_string(), db_password.to_string());
    vars.insert("ROOT_PASSWORD".to_string(), root_password.to_string());
    vars
}

/// Genera las variables para un stack de Kamples.
pub fn kamples_vars(
    domain: &str,
    db_password: &str,
    root_password: &str,
    pg_password: &str,
    glory_branch: &str,
) -> HashMap<String, String> {
    let mut vars = wordpress_vars(domain, db_password, root_password);
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
pub fn rust_vars(domain: &str, db_password: &str, jwt_secret: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    vars.insert("DOMAIN".to_string(), domain.to_string());
    vars.insert("DB_PASSWORD".to_string(), db_password.to_string());
    vars.insert("JWT_SECRET".to_string(), jwt_secret.to_string());
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

        let vars = wordpress_vars("https://blog.com", "secret123", "rootpass");
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
        let vars = wordpress_vars("d", "p", "r");
        assert!(vars.contains_key("DOMAIN"));
        assert!(vars.contains_key("DB_PASSWORD"));
        assert!(vars.contains_key("ROOT_PASSWORD"));
    }

    #[test]
    fn test_kamples_vars_includes_pg() {
        let vars = kamples_vars("https://kamples.com", "p", "r", "pg", "main-kamples");
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
}
