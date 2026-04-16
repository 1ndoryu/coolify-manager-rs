/*
 * Comando: sync-env
 * Sincroniza variables de entorno entre el archivo .env local y el servicio en Coolify.
 *
 * Direcciones:
 *   diff  — muestra diferencias sin aplicar cambios (por defecto)
 *   push  — sube variables locales a Coolify (upsert via API)
 *   pull  — descarga variables de Coolify al archivo .env local
 *
 * El archivo .env local se busca en:
 *   1. --env-file especificado por el usuario
 *   2. Directorio raiz del proyecto (carpeta padre de config/)
 *   3. Directorio de trabajo actual
 *
 * Gotcha: las variables del contenedor en produccion pueden incluir vars del sistema
 * (PATH, HOME, etc.) que no existen en el .env local — son filtradas en el diff.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::validation;

use colored::Colorize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/* ---------- tipos internos ----------------------------------------- */

#[derive(Debug)]
enum DiffStatus {
    LocalOnly,
    RemoteOnly,
    Changed,
    Same,
}

struct EnvDiff {
    key: String,
    local: Option<String>,
    remote: Option<String>,
    status: DiffStatus,
}

/* ---------- punto de entrada ---------------------------------------- */

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    direction: &str,
    dry_run: bool,
    env_file: Option<&Path>,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    let stack_uuid = site.stack_uuid.as_deref().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{site_name}' sin stackUuid configurado"))
    })?;

    let target = settings.resolve_site_target(site)?;
    let api = CoolifyApiClient::new(&target.coolify)?;

    /* Resolver ruta del .env local */
    let local_path = resolve_env_path(config_path, env_file);

    println!("Sitio:      {site_name} ({stack_uuid})");
    println!("Env local:  {}", local_path.display());
    println!("Direccion:  {direction}");
    if dry_run {
        println!("{}", "[DRY RUN — no se aplican cambios]".yellow().bold());
    }
    println!();

    /* Leer env local */
    let local_vars = read_env_file(&local_path)?;

    /* Obtener env remoto via Coolify API */
    let remote_raw = api.get_service_envs(stack_uuid).await?;
    let remote_vars = parse_coolify_envs(&remote_raw);

    /* Calcular diff */
    let diffs = compute_diff(&local_vars, &remote_vars);

    /* Mostrar diff */
    print_diff(&diffs);

    /* Aplicar segun direccion */
    match direction {
        "diff" => { /* solo mostrar */ }
        "push" => {
            let changed: Vec<(String, String)> = diffs
                .iter()
                .filter(|d| matches!(d.status, DiffStatus::LocalOnly | DiffStatus::Changed))
                .map(|d| (d.key.clone(), d.local.clone().unwrap_or_default()))
                .collect();

            if changed.is_empty() {
                println!("{}", "No hay cambios que subir.".green());
            } else if dry_run {
                println!(
                    "{}",
                    format!("[dry-run] Se subirian {} variable(s) a Coolify.", changed.len())
                        .yellow()
                );
            } else {
                api.push_service_envs(stack_uuid, &changed).await?;
                println!(
                    "{}",
                    format!(
                        "{} variable(s) actualizadas en Coolify. Redeploy necesario para aplicar.",
                        changed.len()
                    )
                    .green()
                    .bold()
                );
            }
        }
        "pull" => {
            if dry_run {
                println!(
                    "{}",
                    format!(
                        "[dry-run] Se escribirian {} variable(s) remotas al archivo local.",
                        remote_vars.len()
                    )
                    .yellow()
                );
            } else {
                write_env_file(&local_path, &remote_vars)?;
                println!(
                    "{}",
                    format!(
                        "{} variable(s) escritas en {}",
                        remote_vars.len(),
                        local_path.display()
                    )
                    .green()
                    .bold()
                );
            }
        }
        other => {
            return Err(CoolifyError::Validation(format!(
                "Direccion desconocida '{other}'. Usar: diff, push o pull"
            )));
        }
    }

    Ok(())
}

/* ---------- helpers locales ----------------------------------------- */

fn resolve_env_path(config_path: &Path, env_file: Option<&Path>) -> PathBuf {
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

fn read_env_file(path: &Path) -> std::result::Result<HashMap<String, String>, CoolifyError> {
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

fn parse_env_content(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        /* ignorar comentarios y lineas vacias */
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(eq_pos) = trimmed.find('=') {
            let key = trimmed[..eq_pos].trim().to_string();
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
    if (s.starts_with('"') && s.ends_with('"'))
        || (s.starts_with('\'') && s.ends_with('\''))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn parse_coolify_envs(raw: &[serde_json::Value]) -> HashMap<String, String> {
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

fn compute_diff(
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
    k.contains("SECRET") || k.contains("PASSWORD") || k.contains("TOKEN") || k.contains("KEY") || k.contains("PASS")
}

fn print_diff(diffs: &[EnvDiff]) {
    let changed: Vec<_> = diffs
        .iter()
        .filter(|d| !matches!(d.status, DiffStatus::Same))
        .collect();

    if changed.is_empty() {
        println!("{}", "Local y remoto estan sincronizados.".green().bold());
        return;
    }

    let same_count = diffs.iter().filter(|d| matches!(d.status, DiffStatus::Same)).count();
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
                let display = if is_secret { mask_value(val) } else { val.to_string() };
                println!("  {} {} = {}", "+".green().bold(), d.key.green(), display.green());
                println!("    {} (solo local — no existe en Coolify)", "?".yellow());
            }
            DiffStatus::RemoteOnly => {
                let val = d.remote.as_deref().unwrap_or("");
                let display = if is_secret { mask_value(val) } else { val.to_string() };
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

fn write_env_file(
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
}
