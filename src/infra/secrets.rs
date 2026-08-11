/*
 * Manejo seguro de credenciales y secrets.
 * Usa secrecy para enmascarar valores sensibles en logs/debug.
 * [257B-1] Extensión: redacción automática de env vars y textos multilinea.
 */

use secrecy::SecretString;
use std::collections::HashMap;

/// Enmascara un string para logging seguro (muestra solo primeros 4 chars).
pub fn mask_secret(value: &str) -> String {
    if value.len() <= 4 {
        return "****".to_string();
    }
    format!("{}...{}", &value[..4], "*".repeat(value.len().min(8) - 4))
}

/// Obtiene un secret de variable de entorno como SecretString.
pub fn env_secret(var_name: &str) -> Option<SecretString> {
    std::env::var(var_name).ok().map(SecretString::from)
}

/// Keys que contienen secretos según su nombre.
const SENSITIVE_KEY_PATTERNS: &[&str] = &[
    "PASSWORD",
    "SECRET",
    "TOKEN",
    "KEY",
    "API_KEY",
    "JWT",
    "DATABASE_URL",
    "SMTP_",
    "REDIS_URL",
    "WEBHOOK",
    "CREDENTIALS",
    "PRIVATE",
    "AUTH",
    "SIGNING",
];

/// Determina si una key de variable de entorno es sensible.
pub fn is_sensitive_key(key: &str) -> bool {
    let upper = key.to_uppercase();
    SENSITIVE_KEY_PATTERNS.iter().any(|p| upper.contains(p))
}

/// Redacta URLs con credenciales: postgres://user:pass@host → postgres://[REDACTED]@[REDACTED]
///
/// Maneja correctamente passwords que contienen '@' buscando el ÚLTIMO '@'
/// antes del path (primer '/' después de '://').
fn redact_url_credentials(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        if i + 2 < len && chars[i] == ':' && chars[i + 1] == '/' && chars[i + 2] == '/' {
            let scheme_start = text[..i]
                .rfind(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '(')
                .map(|p| p + 1)
                .unwrap_or(0);
            result.push_str(&text[scheme_start..i + 3]);
            i += 3;
            /* Buscar el final de la authority: '/' que marca el inicio del path,
             * o '?' que marca el query string, o fin del string */
            let authority_end = text[i..]
                .find(['/', '?', '#'])
                .map(|p| i + p)
                .unwrap_or(len);
            /* Buscar el ÚLTIMO '@' dentro de la authority para separar credenciales del host */
            let authority = &text[i..authority_end];
            if let Some(last_at) = authority.rfind('@') {
                let abs_at = i + last_at;
                result.push_str("[REDACTED]");
                result.push('@');
                i = abs_at + 1;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

/// Redacta un mapa de variables de entorno, reemplazando valores sensibles.
pub fn redact_env_map(env: &HashMap<String, String>) -> HashMap<String, String> {
    env.iter()
        .map(|(k, v)| {
            if is_sensitive_key(k) {
                (k.clone(), "[REDACTED]".to_string())
            } else {
                (k.clone(), redact_url_credentials(v))
            }
        })
        .collect()
}

/// Redacta texto multilinea que pueda contener secretos.
/// Aplica redacción de URLs con credenciales y enmascara strings
/// que parecen tokens largos (>32 chars alfanuméricos sin espacios).
pub fn redact_text(text: &str) -> String {
    let mut result = redact_url_credentials(text);
    let mut tokens_to_redact: Vec<String> = Vec::new();
    for word in
        result.split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == ';')
    {
        if word.len() > 32
            && word
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && !word.starts_with("20")
        {
            tokens_to_redact.push(word.to_string());
        }
    }
    for token in &tokens_to_redact {
        result = result.replace(token, &mask_secret(token));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn test_mask_short() {
        assert_eq!(mask_secret("ab"), "****");
        assert_eq!(mask_secret("abcd"), "****");
    }

    #[test]
    fn test_mask_long() {
        let masked = mask_secret("my-secret-token");
        assert!(masked.starts_with("my-s"));
        assert!(masked.contains("*"));
        assert!(!masked.contains("secret-token"));
    }

    #[test]
    fn test_env_secret_missing() {
        std::env::remove_var("CM_TEST_NONEXISTENT_SECRET");
        assert!(env_secret("CM_TEST_NONEXISTENT_SECRET").is_none());
    }

    #[test]
    fn test_env_secret_present() {
        std::env::set_var("CM_TEST_SECRET_VAL", "my-value");
        let secret = env_secret("CM_TEST_SECRET_VAL");
        assert!(secret.is_some());
        assert_eq!(secret.unwrap().expose_secret(), "my-value");
        std::env::remove_var("CM_TEST_SECRET_VAL");
    }

    #[test]
    fn test_is_sensitive_key() {
        assert!(is_sensitive_key("DATABASE_URL"));
        assert!(is_sensitive_key("JWT_SECRET"));
        assert!(is_sensitive_key("SMTP_PASSWORD"));
        assert!(is_sensitive_key("MY_API_KEY"));
        assert!(!is_sensitive_key("APP_NAME"));
        assert!(!is_sensitive_key("PORT"));
    }

    #[test]
    fn test_redact_env_map() {
        let mut env = HashMap::new();
        env.insert("APP_NAME".to_string(), "my-app".to_string());
        env.insert(
            "DATABASE_URL".to_string(),
            "postgres://user:secret@host/db".to_string(),
        );
        env.insert("JWT_SECRET".to_string(), "super-secret-token".to_string());

        let redacted = redact_env_map(&env);
        assert_eq!(redacted["APP_NAME"], "my-app");
        assert_eq!(redacted["DATABASE_URL"], "[REDACTED]");
        assert_eq!(redacted["JWT_SECRET"], "[REDACTED]");
    }

    #[test]
    fn test_redact_url_credentials_simple() {
        let text = "connecting to postgres://user:secret@db.example.com:5432/mydb";
        let result = redact_url_credentials(text);
        assert!(!result.contains("secret"));
        assert!(result.contains("[REDACTED]@db.example.com"));
    }

    #[test]
    fn test_redact_url_credentials_with_at_in_password() {
        let text = "connecting to postgres://admin:p@ssw0rd@db.example.com:5432/mydb";
        let result = redact_url_credentials(text);
        assert!(!result.contains("p@ssw0rd"));
        assert!(!result.contains("admin"));
        assert!(result.contains("[REDACTED]@db.example.com"));
    }

    #[test]
    fn test_redact_url_credentials_no_credentials() {
        let text = "host is db.example.com:5432";
        let result = redact_url_credentials(text);
        assert_eq!(result, text);
    }

    #[test]
    fn test_redact_text_preserves_normal() {
        let text = "server started on port 3000";
        assert_eq!(redact_text(text), text);
    }
}
