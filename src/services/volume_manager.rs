/* [124A-IMAGE404] Gestión de volúmenes persistentes.
 *
 * Coolify normaliza bind mount paths a named volumes en su API interna.
 * Esto causa que las imágenes/uploads desaparezcan después de un redeploy
 * o restart iniciado desde Coolify (UI o API), porque Coolify reescribe
 * el compose en disco con su versión procesada que usa named volumes.
 *
 * Este módulo proporciona funciones para forzar bind mounts en el compose
 * en disco, garantizando que docker compose build/up siempre use el path
 * persistente del host.
 *
 * Gotcha: Coolify procesa compose volumes así:
 *   raw: 'uploads_data:/app/uploads' → processed: 'UUID_uploads-data:/app/uploads'
 *   raw: '/data/uploads/studio:/app/uploads' → processed: 'UUID_uploads-data:/app/uploads'
 * Ambos formatos se normalizan al mismo named volume. No hay forma de evitarlo
 * via API. La solución es parchear el archivo en disco después de que Coolify escriba. */

use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

/* Forzar bind mount para /app/uploads en el compose en disco.
 *
 * Busca cualquier volumen que mapee a /app/uploads (sea named volume o bind mount
 * incorrecto) y lo reemplaza con el bind mount persistente del host.
 *
 * El patrón sed usa [^[:space:]]+ para cubrir todos los formatos de quoting:
 * - Sin comillas: UUID_uploads-data:/app/uploads
 * - Comillas simples: 'UUID_uploads-data:/app/uploads'
 * - Comillas dobles: "UUID_uploads-data:/app/uploads"
 *
 * Coolify puede escribir el compose en disco con o sin comillas dependiendo
 * de la versión y el contexto. El patrón anterior solo cubría comillas simples.
 *
 * Fallback: si no existe ninguna línea :/app/uploads, inserta el bind mount
 * después de la primera sección volumes: del compose (usando awk). */
pub async fn ensure_uploads_bind_mount(
    ssh: &SshClient,
    service_dir: &str,
    site_name: &str,
) -> std::result::Result<(), CoolifyError> {
    let host_path = format!("/data/uploads/{}", site_name);
    let compose_file = format!("{}/docker-compose.yml", service_dir);

    /* Paso 1: sed con patrón amplio que cubre cualquier formato de quoting */
    let fix_cmd = format!(
        "sed -i -E \"s|[^[:space:]]+:/app/uploads[^[:space:]]*|'{host_path}:/app/uploads'|g\" {compose_file}",
    );
    ssh.execute(&fix_cmd).await?;

    let verify = ssh
        .execute(&format!(
            "grep -c '{}:/app/uploads' {}",
            host_path, compose_file
        ))
        .await?;
    let count: i32 = verify.stdout.trim().parse().unwrap_or(0);
    if count > 0 {
        println!("      Bind mount forzado: {}:/app/uploads", host_path);
        return Ok(());
    }

    /* Paso 2 (fallback): no existía ninguna línea :/app/uploads en el compose.
     * Insertar el bind mount después de la primera sección volumes: usando awk.
     * \047 es el octal de comilla simple para evitar problemas de quoting en bash. */
    println!("      No se encontró volumen :/app/uploads — insertando...");
    let fallback_cmd = format!(
        "awk -v bind=\"{host_path}:/app/uploads\" \
         'BEGIN{{f=0}}/volumes:/ && !f{{print; print \"      - \\047\" bind \"\\047\"; f=1; next}}1' \
         {compose_file} > {compose_file}.tmp && mv {compose_file}.tmp {compose_file}",
    );
    ssh.execute(&fallback_cmd).await?;

    /* Re-verificar después del fallback */
    let verify2 = ssh
        .execute(&format!(
            "grep -c '{}:/app/uploads' {}",
            host_path, compose_file
        ))
        .await?;
    let count2: i32 = verify2.stdout.trim().parse().unwrap_or(0);
    if count2 > 0 {
        println!("      Bind mount insertado: {}:/app/uploads", host_path);
        Ok(())
    } else {
        /* Debug: mostrar líneas relevantes para diagnóstico remoto */
        let debug = ssh
            .execute(&format!(
                "grep -n 'volumes\\|uploads\\|/app/' {} || echo 'Sin coincidencias'",
                compose_file
            ))
            .await?;
        Err(CoolifyError::Validation(format!(
            "No se pudo aplicar bind mount para uploads en {}.\n\
             Líneas relevantes del compose:\n{}\n\
             Verificar manualmente el compose en disco.",
            compose_file, debug.stdout
        )))
    }
}

/* [155A-11] Coolify puede guardar envs nuevas en su API pero dejar el
 * docker-compose.yml en disco sin esas claves. Como deploy-service recrea el
 * contenedor con docker compose directo sobre ese archivo, el runtime puede
 * quedar atrasado aunque sync-env diga "sincronizado". Antes del swap,
 * insertar en el compose efectivo las envs runtime faltantes de app. */
pub async fn ensure_runtime_envs_in_compose(
    ssh: &SshClient,
    service_dir: &str,
    app_service_name: &str,
    runtime_envs: &[(String, String)],
) -> std::result::Result<(), CoolifyError> {
    if runtime_envs.is_empty() {
        return Ok(());
    }

    let compose_file = format!("{}/docker-compose.yml", service_dir);
    let compose = ssh.execute(&format!("cat {}", compose_file)).await?;
    if !compose.success() || compose.stdout.trim().is_empty() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo leer {} para sincronizar envs runtime",
            compose_file
        )));
    }

    let sync = upsert_service_environment_entries(&compose.stdout, app_service_name, runtime_envs)?;
    if sync.inserted_keys.is_empty() {
        return Ok(());
    }

    let unique_suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temp_path = std::env::temp_dir().join(format!(
        "coolify-manager-runtime-env-{}-{}.yml",
        std::process::id(),
        unique_suffix
    ));

    std::fs::write(&temp_path, sync.content).map_err(|error| {
        CoolifyError::Validation(format!(
            "No se pudo escribir compose temporal {}: {}",
            temp_path.display(),
            error
        ))
    })?;

    let upload_result = ssh.upload_file(&temp_path, &compose_file).await;
    let _ = std::fs::remove_file(&temp_path);
    upload_result?;

    println!(
        "      Compose envs runtime sincronizadas: {}",
        sync.inserted_keys.join(", ")
    );

    Ok(())
}

/* Preparar directorio de uploads en el host.
 * Crea la estructura de subdirectorios y establece permisos.
 * chmod 777 porque el contenedor puede correr con UID variable (appuser). */
pub async fn ensure_uploads_host_dir(
    ssh: &SshClient,
    site_name: &str,
) -> std::result::Result<String, CoolifyError> {
    let uploads_host_dir = format!("/data/uploads/{}", site_name);
    ssh.execute(&format!(
        "mkdir -p {uploads_host_dir}/content {uploads_host_dir}/deliverables && chmod -R 777 {uploads_host_dir}"
    ))
    .await?;
    Ok(uploads_host_dir)
}

/* [045A-GUARDRAILS] Después de cualquier restart/swap, validar el mount efectivo
 * del contenedor. Healthcheck OK no basta: el contenedor puede estar sano pero
 * usando un named volume vacío en /app/uploads. */
pub async fn verify_runtime_uploads_bind_mount(
    ssh: &SshClient,
    service_dir: &str,
    app_service_name: &str,
    site_name: &str,
) -> std::result::Result<(), CoolifyError> {
    let host_path = format!("/data/uploads/{}", site_name);
    let container_lookup = ssh
        .execute(&format!(
            "cd {} && docker compose ps -q {} 2>/dev/null || true",
            service_dir, app_service_name
        ))
        .await?;
    let container_id = container_lookup.stdout.trim();
    if container_id.is_empty() {
        return Err(CoolifyError::Validation(format!(
            "No se encontró contenedor activo para '{}' al verificar uploads runtime",
            app_service_name
        )));
    }

    let mounts = ssh
        .execute(&format!(
            "docker inspect {} --format '{{{{range .Mounts}}}}{{{{println .Destination \"|\" .Type \"|\" .Source}}}}{{{{end}}}}'",
            container_id
        ))
        .await?;

    if mounts
        .stdout
        .lines()
        .any(|line| mount_points_app_uploads_to(line, &host_path))
    {
        println!(
            "      Runtime OK: /app/uploads usa bind mount {}",
            host_path
        );
        return Ok(());
    }

    Err(CoolifyError::Validation(format!(
        "ABORT: contenedor '{}' no usa bind mount '{}' en /app/uploads. Mounts detectados:\n{}",
        container_id,
        host_path,
        mounts.stdout.trim()
    )))
}

fn mount_points_app_uploads_to(line: &str, host_path: &str) -> bool {
    let mut parts = line.split('|').map(str::trim);
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some("/app/uploads"), Some("bind"), Some(source)) if source == host_path
    )
}

struct ComposeEnvSync {
    content: String,
    inserted_keys: Vec<String>,
}

fn upsert_service_environment_entries(
    compose: &str,
    service_name: &str,
    runtime_envs: &[(String, String)],
) -> std::result::Result<ComposeEnvSync, CoolifyError> {
    if runtime_envs.is_empty() {
        return Ok(ComposeEnvSync {
            content: compose.to_string(),
            inserted_keys: Vec::new(),
        });
    }

    let had_trailing_newline = compose.ends_with('\n');
    let mut lines: Vec<String> = compose.lines().map(ToString::to_string).collect();
    let service_marker = format!("{}:", service_name);
    let service_idx = lines
        .iter()
        .position(|line| line.trim() == service_marker)
        .ok_or_else(|| {
            CoolifyError::Validation(format!(
                "No se encontró el servicio '{}' en docker-compose.yml",
                service_name
            ))
        })?;
    let service_indent = leading_space_count(&lines[service_idx]);
    let service_end =
        find_block_end(&lines, service_idx + 1, service_indent).unwrap_or(lines.len());

    let environment_idx =
        (service_idx + 1..service_end).find(|index| lines[*index].trim() == "environment:");

    let sync = if let Some(environment_idx) = environment_idx {
        let env_indent = leading_space_count(&lines[environment_idx]);
        let env_end =
            find_block_end(&lines, environment_idx + 1, env_indent).unwrap_or(service_end);
        let entry_indent =
            detect_environment_entry_indent(&lines, environment_idx + 1, env_end, env_indent)
                .unwrap_or(env_indent + 4);

        let existing_keys: HashSet<String> = (environment_idx + 1..env_end)
            .filter_map(|index| parse_environment_key(&lines[index], env_indent))
            .collect();

        let missing_envs = missing_runtime_envs(runtime_envs, &existing_keys);
        if missing_envs.is_empty() {
            ComposeEnvSync {
                content: compose.to_string(),
                inserted_keys: Vec::new(),
            }
        } else {
            let insert_at = env_end;
            let inserted_keys = missing_envs
                .iter()
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            let rendered_lines = missing_envs
                .iter()
                .map(|(key, value)| {
                    format!(
                        "{}{}: {}",
                        " ".repeat(entry_indent),
                        key,
                        yaml_single_quote(value)
                    )
                })
                .collect::<Vec<_>>();
            lines.splice(insert_at..insert_at, rendered_lines);

            ComposeEnvSync {
                content: rebuild_compose_text(&lines, had_trailing_newline),
                inserted_keys,
            }
        }
    } else {
        let env_indent = service_indent + 4;
        let entry_indent = env_indent + 4;
        let inserted_keys = runtime_envs
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        let mut rendered_lines = Vec::with_capacity(runtime_envs.len() + 1);
        rendered_lines.push(format!("{}environment:", " ".repeat(env_indent)));
        rendered_lines.extend(runtime_envs.iter().map(|(key, value)| {
            format!(
                "{}{}: {}",
                " ".repeat(entry_indent),
                key,
                yaml_single_quote(value)
            )
        }));
        lines.splice(service_end..service_end, rendered_lines);

        ComposeEnvSync {
            content: rebuild_compose_text(&lines, had_trailing_newline),
            inserted_keys,
        }
    };

    Ok(sync)
}

fn leading_space_count(line: &str) -> usize {
    line.chars()
        .take_while(|character| *character == ' ')
        .count()
}

fn find_block_end(lines: &[String], start_index: usize, parent_indent: usize) -> Option<usize> {
    for (index, line) in lines.iter().enumerate().skip(start_index) {
        if line.trim().is_empty() {
            continue;
        }

        if leading_space_count(line) <= parent_indent {
            return Some(index);
        }
    }

    None
}

fn detect_environment_entry_indent(
    lines: &[String],
    start_index: usize,
    end_index: usize,
    env_indent: usize,
) -> Option<usize> {
    (start_index..end_index).find_map(|index| {
        let line = &lines[index];
        if line.trim().is_empty() {
            return None;
        }

        let indent = leading_space_count(line);
        (indent > env_indent).then_some(indent)
    })
}

fn parse_environment_key(line: &str, env_indent: usize) -> Option<String> {
    if line.trim().is_empty() {
        return None;
    }

    let indent = leading_space_count(line);
    if indent <= env_indent {
        return None;
    }

    line.trim()
        .split_once(':')
        .map(|(key, _)| key.trim().to_string())
}

fn missing_runtime_envs(
    runtime_envs: &[(String, String)],
    existing_keys: &HashSet<String>,
) -> Vec<(String, String)> {
    runtime_envs
        .iter()
        .filter(|(key, _)| !existing_keys.contains(key))
        .cloned()
        .collect()
}

fn yaml_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn rebuild_compose_text(lines: &[String], had_trailing_newline: bool) -> String {
    let mut content = lines.join("\n");
    if had_trailing_newline {
        content.push('\n');
    }
    content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_parser_accepts_expected_bind_mount() {
        assert!(mount_points_app_uploads_to(
            "/app/uploads | bind | /data/uploads/studio",
            "/data/uploads/studio"
        ));
    }

    #[test]
    fn mount_parser_rejects_named_volume() {
        assert!(!mount_points_app_uploads_to(
            "/app/uploads | volume | /var/lib/docker/volumes/demo/_data",
            "/data/uploads/studio"
        ));
    }

    #[test]
    fn upsert_service_environment_entries_inserts_missing_runtime_env() {
        let compose = r#"services:
    app:
        image: demo
        environment:
            GLORY_ADMIN_EMAILS: 'admin@example.com'
        depends_on:
            postgres:
                condition: service_healthy
    postgres:
        image: postgres:16
"#;

        let sync = upsert_service_environment_entries(
            compose,
            "app",
            &[(
                "GLORY_TEST_CHECKOUT_EMAILS".to_string(),
                "test@test.com".to_string(),
            )],
        )
        .expect("compose env sync should succeed");

        assert_eq!(
            sync.inserted_keys,
            vec!["GLORY_TEST_CHECKOUT_EMAILS".to_string()]
        );
        assert!(sync
            .content
            .contains("GLORY_TEST_CHECKOUT_EMAILS: 'test@test.com'"));
        assert!(sync.content.contains("depends_on:"));
    }

    #[test]
    fn upsert_service_environment_entries_does_not_duplicate_existing_key() {
        let compose = r#"services:
    app:
        image: demo
        environment:
            GLORY_TEST_CHECKOUT_EMAILS: 'test@test.com'
"#;

        let sync = upsert_service_environment_entries(
            compose,
            "app",
            &[(
                "GLORY_TEST_CHECKOUT_EMAILS".to_string(),
                "test@test.com".to_string(),
            )],
        )
        .expect("compose env sync should succeed");

        assert!(sync.inserted_keys.is_empty());
        assert_eq!(sync.content, compose);
    }

    #[test]
    fn yaml_single_quote_escapes_single_quotes() {
        assert_eq!(yaml_single_quote("it'works"), "'it''works'");
    }
}
