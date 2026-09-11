use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;

pub(crate) struct BuildEnv {
    pub(crate) shell_prefix: String,
    pub(crate) build_arg_flags: String,
    pub(crate) count: usize,
}

pub(crate) async fn build_env_from_coolify(
    coolify_config: &crate::config::CoolifyConfig,
    stack_uuid: &str,
) -> std::result::Result<BuildEnv, CoolifyError> {
    let api = CoolifyApiClient::new(coolify_config)?;
    let envs = api.get_service_envs(stack_uuid).await?;
    let mut assignments = Vec::new();
    let mut build_args = Vec::new();

    for env in envs {
        let Some(key) = env.get("key").and_then(|v| v.as_str()) else {
            continue;
        };
        if !key.starts_with("VITE_") || !is_safe_shell_env_key(key) {
            continue;
        }
        let value = env
            .get("real_value")
            .and_then(|v| v.as_str())
            .or_else(|| env.get("value").and_then(|v| v.as_str()))
            .unwrap_or("");
        if value.trim().is_empty() {
            continue;
        }
        let escaped_value = escape_shell_single_quote(value);
        assignments.push(format!("{key}='{escaped_value}'"));
        build_args.push(format!("--build-arg {key}='{escaped_value}'"));
    }

    assignments.sort();
    build_args.sort();
    let count = assignments.len();
    let shell_prefix = if assignments.is_empty() {
        String::new()
    } else {
        format!("{} ", assignments.join(" "))
    };
    Ok(BuildEnv {
        shell_prefix,
        build_arg_flags: build_args.join(" "),
        count,
    })
}

pub(crate) async fn runtime_envs_from_coolify(
    coolify_config: &crate::config::CoolifyConfig,
    stack_uuid: &str,
) -> std::result::Result<Vec<(String, String)>, CoolifyError> {
    let api = CoolifyApiClient::new(coolify_config)?;
    let envs = api.get_service_envs(stack_uuid).await?;
    let mut runtime_envs = Vec::new();

    for env in envs {
        let Some(key) = env.get("key").and_then(|value| value.as_str()) else {
            continue;
        };
        if !is_safe_shell_env_key(key) || should_skip_runtime_compose_env(key) {
            continue;
        }
        if env
            .get("is_preview")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            continue;
        }
        if env
            .get("is_build_time")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            continue;
        }

        let value = env
            .get("real_value")
            .and_then(|value| value.as_str())
            .or_else(|| env.get("value").and_then(|value| value.as_str()))
            .unwrap_or("")
            .trim();
        if value.is_empty() {
            continue;
        }

        runtime_envs.push((key.to_string(), value.to_string()));
    }

    runtime_envs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(runtime_envs)
}

pub(crate) fn should_skip_runtime_compose_env(key: &str) -> bool {
    (key.starts_with("COOLIFY_") && !is_prefixed_coolify_target_key(key))
        || key.ends_with("_SSH_KEY_PATH")
        || key.starts_with("SERVICE_")
        || key.starts_with("VITE_")
        || key.starts_with("POSTGRES_")
        || matches!(key, "APP_BIN" | "BRANCH" | "REPO_URL")
}

/* [225A-3] Multi-VPS Rust necesita COOLIFY_VPSn_* dentro del runtime.
 * Las claves COOLIFY_* planas siguen fuera del compose porque son de plataforma Coolify. */
pub(crate) fn is_prefixed_coolify_target_key(key: &str) -> bool {
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
            "API_TOKEN" | "BASE_URL" | "PROJECT_UUID" | "SERVER_IP" | "SERVER_UUID"
        )
}

pub(crate) fn is_safe_shell_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

pub(crate) fn escape_shell_single_quote(value: &str) -> String {
    value.replace('\'', "'\\''")
}

pub(crate) fn normalize_health_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        "/".to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}
