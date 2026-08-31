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
 *
 * Los helpers de parsing/diff/politica de push viven en sync_env_helpers.rs (Fase H):
 * este archivo conserva solo la orquestacion del comando.
 */

use super::sync_env_helpers::*;

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::validation;

use colored::Colorize;
use std::collections::HashSet;
use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    direction: &str,
    dry_run: bool,
    env_file: Option<&Path>,
    only_keys: &[String],
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
    println!("Env base:   {}", local_path.display());
    println!("Direccion:  {direction}");
    if dry_run {
        println!("{}", "[DRY RUN — no se aplican cambios]".yellow().bold());
    }
    println!();

    /* Leer env local. Para apps Vite, tambien mergea frontend/.env porque esas
     * variables se consumen en build-time y suelen vivir fuera del .env backend. */
    let local_bundle = read_env_bundle(&local_path)?;
    if local_bundle.files.len() > 1 {
        println!(
            "Env extra:  {}",
            local_bundle.files[1..]
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    for derived in &local_bundle.derived {
        println!("Derivada:   {derived}");
    }
    let local_vars = local_bundle.vars;
    let only_filter = normalize_only_keys(only_keys);
    if !only_filter.is_empty() {
        println!(
            "Filtro:    solo {}",
            only_filter.iter().cloned().collect::<Vec<_>>().join(", ")
        );
    }

    /* Obtener env remoto via Coolify API */
    let remote_raw = api.get_service_envs(stack_uuid).await?;
    let remote_vars = parse_coolify_envs(&remote_raw);

    /* Calcular diff */
    let diffs = compute_diff(&local_vars, &remote_vars);
    let operation_diffs = filter_diffs(&diffs, &only_filter);

    if !only_filter.is_empty() {
        let known_keys: HashSet<&str> = diffs.iter().map(|d| d.key.as_str()).collect();
        let missing_requested: Vec<&str> = only_filter
            .iter()
            .map(String::as_str)
            .filter(|key| !known_keys.contains(key))
            .collect();
        if !missing_requested.is_empty() {
            return Err(CoolifyError::Validation(format!(
                "Las variables pedidas con --only no existen en local ni remoto: {}",
                missing_requested.join(", ")
            )));
        }
    }

    let required = required_env_status(&site.template, &local_vars, &remote_vars);
    print_required_env_status(&required);

    /* Mostrar diff */
    print_diff(&operation_diffs);

    /* Aplicar segun direccion */
    match direction {
        "diff" => { /* solo mostrar */ }
        "push" => {
            let missing_local: Vec<&str> = required
                .iter()
                .filter(|r| !r.local_present)
                .map(|r| r.key)
                .collect();
            if !missing_local.is_empty() {
                return Err(CoolifyError::Validation(format!(
                    "Faltan variables requeridas en local: {}",
                    missing_local.join(", ")
                )));
            }

            let changed: Vec<(String, String)> = operation_diffs
                .iter()
                .filter(|d| matches!(d.status, DiffStatus::LocalOnly | DiffStatus::Changed))
                .map(|d| (d.key.clone(), d.local.clone().unwrap_or_default()))
                .collect();

            /* [25A-DB-AUTH] Bloquear variables gestionadas por Coolify:
             * SERVICE_PASSWORD_*, SERVICE_NAME_*, SERVICE_FQDN_*, SERVICE_URL_*
             * y las variables de runtime que el compose renderiza de forma controlada.
             * Subirlas fuerza a Coolify a regenerarlas en el siguiente deploy, lo que
             * causa mismatch de credenciales entre DATABASE_URL y el volumen de postgres. */
            let blocked: Vec<&str> = changed
                .iter()
                .filter(|(k, _)| is_blocked_push_key(k))
                .map(|(k, _)| k.as_str())
                .collect();
            if !blocked.is_empty() {
                eprintln!(
                    "{}",
                    format!(
                        "WARN: Variables gestionadas por Coolify BLOQUEADAS (no se subiran):\n       {}",
                        blocked.join(", ")
                    )
                    .yellow()
                    .bold()
                );
                eprintln!("      Subirlas puede romper DB/JWT/rutas renderizadas por el compose.");
            }
            let skipped: Vec<&str> = changed
                .iter()
                .filter(|(k, _)| !is_blocked_push_key(k) && !is_allowed_push_key(&site.template, k))
                .map(|(k, _)| k.as_str())
                .collect();
            if !skipped.is_empty() {
                eprintln!(
                    "{}",
                    format!(
                        "INFO: Variables locales fuera de la politica del stack (no se subiran):\n       {}",
                        skipped.join(", ")
                    )
                    .cyan()
                );
            }

            let changed: Vec<(String, String)> = changed
                .into_iter()
                .filter(|(k, _)| !is_blocked_push_key(k) && is_allowed_push_key(&site.template, k))
                .collect();

            if changed.is_empty() {
                println!("{}", "No hay cambios que subir.".green());
            } else if dry_run {
                println!(
                    "{}",
                    format!(
                        "[dry-run] Se subirian {} variable(s) a Coolify.",
                        changed.len()
                    )
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
            if !only_filter.is_empty() {
                return Err(CoolifyError::Validation(
                    "--only solo esta soportado con direction=diff o direction=push".to_string(),
                ));
            }
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
