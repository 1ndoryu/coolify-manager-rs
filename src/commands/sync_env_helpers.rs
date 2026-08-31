/*
 * Helpers de sync-env: parsing de archivos .env, diff local/remoto,
 * politicas de push (blocklist/allowlist) y rendering de salida.
 * Extraido de sync_env.rs (Fase H): helpers sin I/O remoto para mantener
 * el comando (orquestacion) bajo el limite de lineas.
 */

use crate::domain::StackTemplate;
use crate::error::CoolifyError;

use colored::Colorize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum DiffStatus {
    LocalOnly,
    RemoteOnly,
    Changed,
    Same,
}

#[derive(Clone)]
pub struct EnvDiff {
    pub key: String,
    pub local: Option<String>,
    pub remote: Option<String>,
    pub status: DiffStatus,
}

pub struct LocalEnvBundle {
    pub vars: HashMap<String, String>,
    pub files: Vec<PathBuf>,
    pub derived: Vec<String>,
}

pub struct RequiredEnvStatus {
    pub key: &'static str,
    pub scope: &'static str,
    pub local_present: bool,
    pub remote_present: bool,
}

pub fn resolve_env_path(config_path: &Path, env_file: Option<&Path>) -> PathBuf {
    if let Some(p) = env_file {
        return p.to_path_buf();
    }
    /* Subir dos niveles desde config/settings.json → raiz del proyecto */
    if let Some(parent) = config_path.parent().and_then(|p| p.parent()) {
        let candidate = parent.join(".env");
        if candidate.exists() {
            return candidate;
        }
        let candidate_local = parent.join(".env.local");
        if candidate_local.exists() {
            return candidate_local;
        }
    }
    /* Fallback: cwd */
    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join(".env");
        if candidate.exists() {
            return candidate;
        }
    }
    /* Retornar ruta por defecto aunque no exista */
    config_path
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join(".env"))
        .unwrap_or_else(|| PathBuf::from(".env"))
}

pub fn read_env_bundle(path: &Path) -> std::result::Result<LocalEnvBundle, CoolifyError> {
    let mut vars = read_env_file(path)?;
    let mut files = vec![path.to_path_buf()];
    let mut derived = Vec::new();

    if let Some(project_root) = project_root_for_env(path) {
        for candidate in [
            project_root.join("frontend/.env"),
            project_root.join("frontend/.env.local"),
        ] {
            if candidate.exists() && candidate != path {
                let extra = read_env_file(&candidate)?;
                vars.extend(extra);
                files.push(candidate);
            }
        }
    }

    if !vars.contains_key("VITE_STRIPE_PUBLISHABLE_KEY") {
        if let Some(value) = vars.get("GLORY_STRIPE_PUBLISHABLE_KEY").cloned() {
            vars.insert("VITE_STRIPE_PUBLISHABLE_KEY".to_string(), value);
            derived.push("VITE_STRIPE_PUBLISHABLE_KEY <- GLORY_STRIPE_PUBLISHABLE_KEY".to_string());
        }
    }

    Ok(LocalEnvBundle {
        vars,
        files,
        derived,
    })
}

fn project_root_for_env(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    if parent.file_name().and_then(|n| n.to_str()) == Some("frontend") {
        return parent.parent().map(Path::to_path_buf);
    }
    Some(parent.to_path_buf())
}

pub fn read_env_file(path: &Path) -> std::result::Result<HashMap<String, String>, CoolifyError> {
    if !path.exists() {
        return Err(CoolifyError::Validation(format!(
            "Archivo .env no encontrado: {}",
            path.display()
        )));
    }
    let content = std::fs::read_to_string(path).map_err(|e| {
        CoolifyError::Validation(format!("No se pudo leer {}: {e}", path.display()))
    })?;
    Ok(parse_env_content(&content))
}

pub fn normalize_only_keys(keys: &[String]) -> HashSet<String> {
    keys.iter()
        .flat_map(|key| key.split(','))
        .map(|key| key.trim().trim_start_matches('\u{feff}').to_string())
        .filter(|key| !key.is_empty())
        .collect()
}

pub fn filter_diffs(diffs: &[EnvDiff], only_filter: &HashSet<String>) -> Vec<EnvDiff> {
    diffs
        .iter()
        .filter(|diff| only_filter.is_empty() || only_filter.contains(&diff.key))
        .cloned()
        .collect()
}

pub fn required_env_status(
    template: &StackTemplate,
    local: &HashMap<String, String>,
    remote: &HashMap<String, String>,
) -> Vec<RequiredEnvStatus> {
    required_env_keys(template)
        .into_iter()
        .map(|(key, scope)| RequiredEnvStatus {
            key,
            scope,
            local_present: local.get(key).is_some_and(|v| !v.trim().is_empty()),
            remote_present: remote.get(key).is_some_and(|v| !v.trim().is_empty()),
        })
        .collect()
}

fn required_env_keys(template: &StackTemplate) -> Vec<(&'static str, &'static str)> {
    match template {
        StackTemplate::Rust => vec![
            ("VITE_STRIPE_PUBLISHABLE_KEY", "frontend build"),
            ("GLORY_STRIPE_SECRET_KEY", "backend runtime"),
            ("GLORY_STRIPE_WEBHOOK_SECRET", "backend runtime"),
        ],
        _ => Vec::new(),
    }
}

pub fn print_required_env_status(statuses: &[RequiredEnvStatus]) {
    if statuses.is_empty() {
        return;
    }
    println!("Variables requeridas:");
    for status in statuses {
        let local = if status.local_present {
            "local OK".green()
        } else {
            "local FALTA".red()
        };
        let remote = if status.remote_present {
            "Coolify OK".green()
        } else {
            "Coolify FALTA".red()
        };
        println!(
            "  - {} ({}) — {}, {}",
            status.key, status.scope, local, remote
        );
    }
    println!();
}

pub fn is_blocked_push_key(key: &str) -> bool {
    key.starts_with("SERVICE_")
        || matches!(
            key,
            "DATABASE_URL" | "JWT_SECRET" | "HOST" | "PORT" | "STATIC_DIR" | "SQLX_OFFLINE"
        )
}

pub fn is_allowed_push_key(template: &StackTemplate, key: &str) -> bool {
    match template {
        StackTemplate::Rust => {
            RUST_PUSH_ALLOWLIST.contains(&key) || is_prefixed_coolify_target_key(key)
        }
        _ => true,
    }
}

/* [225A-5] El panel de infraestructura necesita COOLIFY_VPSn_* completos en runtime.
 * Mantener esto generico evita repetir la deuda VPS1/VPS2 cuando se agregue otra VPS. */
fn is_prefixed_coolify_target_key(key: &str) -> bool {
    let Some(rest) = key.strip_prefix("COOLIFY_VPS") else {
        return false;
    };
    let Some((index, suffix)) = rest.split_once('_') else {
        return false;
    };

    !index.is_empty()
        && index.chars().all(|ch| ch.is_ascii_digit())
        && matches!(
            suffix,
            "API_TOKEN" | "BASE_URL" | "PROJECT_UUID" | "SERVER_IP" | "SERVER_UUID" /* [235A-2] SSH_KEY_PATH ya no se empuja al compose: son paths locales (Windows)
                                                                                     * que no existen en el contenedor Linux. El template provee el path correcto. */
        )
}

const RUST_PUSH_ALLOWLIST: &[&str] = &[
    "AI_API_URL",
    "AI_MODEL",
    "AI_RELEVANCE_ENABLED",
    "AI_ROTATION_ENABLED",
    "ALLOW_DUPLICATE_UPLOADS",
    "APP_URL",
    "BDP_BASE_URL",
    "BDP_BOOTSTRAP_USER_EMAIL",
    "BDP_DEFAULT_ARTICLE_CODE",
    "BDP_DEFAULT_ARTICLE_NAME",
    "BDP_EMPLOYEE_ID",
    "BDP_INTEGRATOR_CODE",
    "BDP_ITEMS_PROFILE_ID",
    "BDP_LOGIN",
    "BDP_PASSWORD",
    "BDP_POS_ID",
    "CHAT_WHATSAPP_DELIVERY_ENABLED",
    "CONTABO_API_PASSWORD",
    "CONTABO_API_USER",
    "CONTABO_CLIENT_ID",
    "CONTABO_CLIENT_SECRET",
    "CONTABO_DEFAULT_IMAGE_ID",
    "COOLIFY_API_TOKEN",
    "COOLIFY_BASE_URL",
    "COOLIFY_PROJECT_UUID",
    "COOLIFY_SERVER_IP",
    "COOLIFY_SERVER_UUID",
    /* [235A-2] COOLIFY_SSH_KEY_PATH no se empuja: es path local Windows. */
    "DEEPSEEK_API",
    "DEEPSEEK_API_KEY",
    "DEEPSEEK_API_URL",
    "DEEPSEEK_MODEL",
    "ERROR_REPORT_EMAIL",
    "FFMPEG_PATH",
    "FFPROBE_PATH",
    "FIXTURES_SYNC",
    "GEMINI_API_URL",
    "GLORY_ALERT_GATEWAY_URL",
    "GLORY_ALLOWED_ORIGINS",
    "GLORY_ADMIN_EMAILS",
    "GLORY_INTERNAL_ALERT_SECRET",
    "GLORY_PUBLIC_URL",
    "GLORY_SMTP_HOST",
    "GLORY_SMTP_PASSWORD",
    "GLORY_SMTP_PORT",
    "GLORY_SMTP_USER",
    "GLORY_STRIPE_HOSTING_PRICE_BASICO",
    "GLORY_STRIPE_HOSTING_PRICE_ECOMMERCE",
    "GLORY_STRIPE_HOSTING_PRICE_PRO",
    "GLORY_STRIPE_PRICE_PRO",
    "GLORY_STRIPE_PUBLISHABLE_KEY",
    "GLORY_STRIPE_SECRET_KEY",
    "GLORY_STRIPE_WEBHOOK_SECRET",
    "GLORY_TEST_CHECKOUT_EMAILS",
    "GOOGLE_GEMINI_API",
    "GROQ_API",
    "GROQ_API_1",
    "GROQ_API_2",
    "GROQ_API_3",
    "IMAGES_BASE_URL",
    "IMAGES_STORE_PATH",
    "KAMPLES_CRON_SECRET",
    "KAMPLES_PG_DBNAME",
    "KAMPLES_PG_HOST",
    "KAMPLES_PG_PASSWORD",
    "KAMPLES_PG_PORT",
    "KAMPLES_PG_USER",
    "KAMPLES_SISTEMA_USUARIO_ID",
    "META_ACCESS_TOKEN",
    "META_BUSINESS_APP_ID",
    "META_PHONE_NUMBER_ID",
    "META_WABA_ID",
    "META_WHATSAPP_NUMBER",
    "PUBLIC_URL",
    "SITE_URL",
    "SMTP_BCC",
    "SMTP_FROM",
    "SMTP_FROM_NAME",
    "SMTP_HOST",
    "SMTP_PASS",
    "SMTP_PASSWORD",
    "SMTP_PORT",
    "SMTP_USER",
    "SOUNDCLOUD_OAUTH_TOKEN",
    "STRIPE_SECRET_KEY",
    "STRIPE_WEBHOOK_SECRET",
    "VITE_API_URL",
    "VITE_STRIPE_PUBLISHABLE_KEY",
];

pub fn parse_env_content(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        /* ignorar comentarios y lineas vacias */
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(eq_pos) = trimmed.find('=') {
            let key = trimmed[..eq_pos]
                .trim()
                .trim_start_matches('\u{feff}')
                .to_string();
            let raw_val = trimmed[eq_pos + 1..].trim();
            /* quitar comillas opcionales alrededor del valor */
            let value = strip_quotes(raw_val).to_string();
            if !key.is_empty() {
                map.insert(key, value);
            }
        }
    }
    map
}

fn strip_quotes(s: &str) -> &str {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

pub fn parse_coolify_envs(raw: &[serde_json::Value]) -> HashMap<String, String> {
    raw.iter()
        .filter_map(|v| {
            let key = v.get("key")?.as_str()?.to_string();
            /* usar real_value si existe (sin mascara), fallback a value */
            let value = v
                .get("real_value")
                .and_then(|v| v.as_str())
                .or_else(|| v.get("value")?.as_str())
                .unwrap_or("")
                .to_string();
            Some((key, value))
        })
        .collect()
}

pub fn compute_diff(
    local: &HashMap<String, String>,
    remote: &HashMap<String, String>,
) -> Vec<EnvDiff> {
    let mut keys: Vec<String> = local
        .keys()
        .chain(remote.keys())
        .cloned()
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    keys.sort();

    keys.into_iter()
        .map(|key| {
            let local_val = local.get(&key).cloned();
            let remote_val = remote.get(&key).cloned();
            let status = match (&local_val, &remote_val) {
                (Some(l), Some(r)) if l == r => DiffStatus::Same,
                (Some(_), Some(_)) => DiffStatus::Changed,
                (Some(_), None) => DiffStatus::LocalOnly,
                (None, Some(_)) => DiffStatus::RemoteOnly,
                (None, None) => unreachable!(),
            };
            EnvDiff {
                key,
                local: local_val,
                remote: remote_val,
                status,
            }
        })
        .collect()
}

fn mask_value(v: &str) -> String {
    /* Mostrar solo los primeros 4 chars para vars con aspecto de secret */
    if v.len() > 8 {
        format!("{}****", &v[..4])
    } else {
        "****".to_string()
    }
}

fn looks_secret(key: &str) -> bool {
    let k = key.to_uppercase();
    k.contains("SECRET")
        || k.contains("PASSWORD")
        || k.contains("LOGIN")
        || k.contains("TOKEN")
        || k.contains("KEY")
        || k.contains("PASS")
        || k == "USER"
        || k.ends_with("_USER")
        || k.contains("_USER_")
        || k.ends_with("_API")
        || k.contains("_API_")
        || k.starts_with("API_")
}

pub fn print_diff(diffs: &[EnvDiff]) {
    let changed: Vec<_> = diffs
        .iter()
        .filter(|d| !matches!(d.status, DiffStatus::Same))
        .collect();

    if changed.is_empty() {
        println!("{}", "Local y remoto estan sincronizados.".green().bold());
        return;
    }

    let same_count = diffs
        .iter()
        .filter(|d| matches!(d.status, DiffStatus::Same))
        .count();
    println!(
        "  {} identicas, {} con diferencias:\n",
        same_count,
        changed.len()
    );

    for d in &changed {
        let is_secret = looks_secret(&d.key);
        match d.status {
            DiffStatus::LocalOnly => {
                let val = d.local.as_deref().unwrap_or("");
                let display = if is_secret {
                    mask_value(val)
                } else {
                    val.to_string()
                };
                println!(
                    "  {} {} = {}",
                    "+".green().bold(),
                    d.key.green(),
                    display.green()
                );
                println!("    {} (solo local — no existe en Coolify)", "?".yellow());
            }
            DiffStatus::RemoteOnly => {
                let val = d.remote.as_deref().unwrap_or("");
                let display = if is_secret {
                    mask_value(val)
                } else {
                    val.to_string()
                };
                println!("  {} {} = {}", "-".red().bold(), d.key.red(), display.red());
                println!("    {} (solo remoto — no existe localmente)", "?".yellow());
            }
            DiffStatus::Changed => {
                let lv = d.local.as_deref().unwrap_or("");
                let rv = d.remote.as_deref().unwrap_or("");
                let (dl, dr) = if is_secret {
                    (mask_value(lv), mask_value(rv))
                } else {
                    (lv.to_string(), rv.to_string())
                };
                println!("  {} {}", "~".yellow().bold(), d.key.yellow());
                println!("    local:  {}", dl.yellow());
                println!("    remoto: {}", dr.cyan());
            }
            DiffStatus::Same => {}
        }
    }
    println!();
}

pub fn write_env_file(
    path: &Path,
    vars: &HashMap<String, String>,
) -> std::result::Result<(), CoolifyError> {
    let mut keys: Vec<&String> = vars.keys().collect();
    keys.sort();

    let mut content = String::from("# Generado por coolify-manager sync-env pull\n");
    for k in keys {
        let v = &vars[k];
        /* Envolver en comillas si el valor contiene espacios o caracteres especiales */
        if v.contains(' ') || v.contains('#') || v.contains('"') {
            content.push_str(&format!("{k}=\"{}\"\n", v.replace('"', "\\\"")));
        } else {
            content.push_str(&format!("{k}={v}\n"));
        }
    }

    std::fs::write(path, content).map_err(|e| {
        CoolifyError::Validation(format!("No se pudo escribir {}: {e}", path.display()))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_env_content_basic() {
        let content = "KEY=value\nOTHER = val2 \n# comment\n\nEMPTY=";
        let map = parse_env_content(content);
        assert_eq!(map["KEY"], "value");
        assert_eq!(map["OTHER"], "val2");
        assert!(map.contains_key("EMPTY"));
        assert!(!map.contains_key("comment"));
    }

    #[test]
    fn test_parse_env_content_quotes() {
        let content = "QUOTED=\"hello world\"\nSINGLE='value'";
        let map = parse_env_content(content);
        assert_eq!(map["QUOTED"], "hello world");
        assert_eq!(map["SINGLE"], "value");
    }

    #[test]
    fn test_diff_same() {
        let mut local = HashMap::new();
        let mut remote = HashMap::new();
        local.insert("A".into(), "1".into());
        remote.insert("A".into(), "1".into());
        let diffs = compute_diff(&local, &remote);
        assert!(matches!(diffs[0].status, DiffStatus::Same));
    }

    #[test]
    fn derives_vite_stripe_from_public_backend_key() {
        let temp = tempfile::tempdir().unwrap();
        let env_path = temp.path().join(".env");
        std::fs::write(&env_path, "GLORY_STRIPE_PUBLISHABLE_KEY=pk_live_test\n").unwrap();

        let bundle = read_env_bundle(&env_path).unwrap();
        let local = bundle.vars;
        let remote = HashMap::new();

        let status = required_env_status(&StackTemplate::Rust, &local, &remote);
        let vite = status
            .iter()
            .find(|s| s.key == "VITE_STRIPE_PUBLISHABLE_KEY")
            .unwrap();
        assert!(vite.local_present);
        assert!(!vite.remote_present);
    }

    #[test]
    fn rust_policy_blocks_runtime_infra_and_allows_vite_stripe() {
        assert!(is_blocked_push_key("DATABASE_URL"));
        assert!(is_blocked_push_key("SERVICE_PASSWORD_POSTGRES"));
        assert!(!is_allowed_push_key(
            &StackTemplate::Rust,
            "CLIENT_SECRET_CONTABO"
        ));
        assert!(is_allowed_push_key(
            &StackTemplate::Rust,
            "VITE_STRIPE_PUBLISHABLE_KEY"
        ));
        assert!(is_allowed_push_key(
            &StackTemplate::Rust,
            "GLORY_STRIPE_SECRET_KEY"
        ));
    }

    #[test]
    fn rust_policy_allows_nakomi_whatsapp_gateway_keys() {
        for key in [
            "CHAT_WHATSAPP_DELIVERY_ENABLED",
            "GLORY_ALERT_GATEWAY_URL",
            "GLORY_INTERNAL_ALERT_SECRET",
        ] {
            assert!(
                is_allowed_push_key(&StackTemplate::Rust, key),
                "{key} debe permitirse para configurar el gateway interno de WhatsApp"
            );
        }
    }
}
