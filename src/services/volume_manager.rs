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
    if sync.inserted_keys.is_empty() && sync.updated_keys.is_empty() {
        return Ok(());
    }

    upload_compose_content(ssh, &compose_file, sync.content).await?;

    if !sync.inserted_keys.is_empty() {
        println!(
            "      Compose envs runtime sincronizadas: {}",
            sync.inserted_keys.join(", ")
        );
    }
    if !sync.updated_keys.is_empty() {
        println!(
            "      Compose envs runtime actualizadas: {}",
            sync.updated_keys.join(", ")
        );
    }

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

/* [235A-5] Cuando Coolify reescribe /app/uploads como named volume, el bind real
 * puede seguir teniendo las imágenes antiguas mientras el volumen equivocado acumula
 * subidas nuevas. Antes de recrear el contenedor, fusionar el contenido actual en el
 * bind host con cp -n para preservar ambas fuentes sin sobrescribir. */
pub async fn merge_current_uploads_into_host_bind(
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
        return Ok(());
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
        return Ok(());
    }

    let file_count = ssh
        .execute(&format!(
            "docker exec {} sh -c 'find /app/uploads -type f 2>/dev/null | wc -l'",
            container_id
        ))
        .await?;
    let count: u64 = file_count.stdout.trim().parse().unwrap_or(0);
    if count == 0 {
        return Ok(());
    }

    let merge_cmd = format!(
        "tmp=$(mktemp -d /tmp/cm-uploads-merge.XXXXXX) \
         && docker cp {container_id}:/app/uploads/. \"$tmp/\" \
         && mkdir -p {host_path} \
         && cp -an \"$tmp/.\" {host_path}/ \
         && chmod -R 777 {host_path} \
         && rm -rf \"$tmp\" \
         && echo MERGED",
    );
    let merge = ssh.execute(&merge_cmd).await?;
    if !merge.success() || !merge.stdout.contains("MERGED") {
        return Err(CoolifyError::Validation(format!(
            "No se pudo fusionar uploads desde el contenedor actual: {}{}",
            merge.stdout.trim(),
            merge.stderr.trim()
        )));
    }

    println!(
        "      Uploads del volumen actual fusionados en {} ({} archivos, sin sobrescribir).",
        host_path, count
    );
    Ok(())
}

/* [235A-6] Las rutas SSH guardadas en Coolify pueden venir del equipo local
 * Windows y no existir dentro del contenedor. Si el host tiene una clave
 * `/root/{site}-ssh/id_ed25519`, el compose efectivo monta esa carpeta y usa
 * la ruta Linux esperada por el sampler de infraestructura. */
pub async fn ensure_runtime_ssh_bind_mount(
    ssh: &SshClient,
    service_dir: &str,
    app_service_name: &str,
    site_name: &str,
) -> std::result::Result<(), CoolifyError> {
    let host_ssh_dir = format!("/root/{}-ssh", site_name);
    let host_key_path = format!("{host_ssh_dir}/id_ed25519");
    let key_check = ssh
        .execute(&format!(
            "test -r '{}' && echo PRESENT || echo MISSING",
            host_key_path
        ))
        .await?;
    if !key_check.stdout.contains("PRESENT") {
        return Ok(());
    }

    let vps2_key_path = format!("{host_ssh_dir}/vps2_backup");
    let vps2_key_sync = ssh
        .execute(&format!(
            "mkdir -p '{host_ssh_dir}' && if test -r /root/.ssh/vps2_backup; then cp /root/.ssh/vps2_backup '{vps2_key_path}' && chmod 600 '{vps2_key_path}' && echo VPS2_PRESENT; else echo VPS2_MISSING; fi"
        ))
        .await?;
    let has_vps2_key = vps2_key_sync.stdout.contains("VPS2_PRESENT");

    let compose_file = format!("{}/docker-compose.yml", service_dir);
    let compose = ssh.execute(&format!("cat {}", compose_file)).await?;
    if !compose.success() || compose.stdout.trim().is_empty() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo leer {} para sincronizar mount SSH",
            compose_file
        )));
    }

    let volume_entry = format!("{}:/home/appuser/.ssh", host_ssh_dir);
    let volume_sync =
        ensure_service_volume_entry(&compose.stdout, app_service_name, &volume_entry)?;
    let mut ssh_envs = vec![(
        "COOLIFY_VPS1_SSH_KEY_PATH".to_string(),
        "/home/appuser/.ssh/id_ed25519".to_string(),
    )];
    if has_vps2_key {
        ssh_envs.push((
            "COOLIFY_SSH_KEY_PATH".to_string(),
            "/home/appuser/.ssh/vps2_backup".to_string(),
        ));
    }
    let env_sync = upsert_service_environment_entries(
        &volume_sync.content,
        app_service_name,
        &ssh_envs,
    )?;

    if !volume_sync.changed && env_sync.inserted_keys.is_empty() && env_sync.updated_keys.is_empty()
    {
        return Ok(());
    }

    upload_compose_content(ssh, &compose_file, env_sync.content).await?;
    if volume_sync.changed {
        println!("      Compose mount SSH sincronizado: {volume_entry}");
    }
    if !env_sync.inserted_keys.is_empty() || !env_sync.updated_keys.is_empty() {
        let mut changed_keys = env_sync.inserted_keys;
        changed_keys.extend(env_sync.updated_keys);
        println!("      Compose env SSH sincronizada: {}", changed_keys.join(", "));
    }
    Ok(())
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
    updated_keys: Vec<String>,
}

struct ComposeVolumeSync {
    content: String,
    changed: bool,
}

async fn upload_compose_content(
    ssh: &SshClient,
    compose_file: &str,
    content: String,
) -> std::result::Result<(), CoolifyError> {
    let unique_suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temp_path = std::env::temp_dir().join(format!(
        "coolify-manager-compose-{}-{}.yml",
        std::process::id(),
        unique_suffix
    ));

    std::fs::write(&temp_path, content).map_err(|error| {
        CoolifyError::Validation(format!(
            "No se pudo escribir compose temporal {}: {}",
            temp_path.display(),
            error
        ))
    })?;

    let upload_result = ssh.upload_file(&temp_path, compose_file).await;
    let _ = std::fs::remove_file(&temp_path);
    upload_result
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
            updated_keys: Vec::new(),
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

        let existing_entries: Vec<(usize, String, String)> = (environment_idx + 1..env_end)
            .filter_map(|index| {
                parse_environment_entry(&lines[index], env_indent).map(|(k, v)| (index, k, v))
            })
            .collect();
        let existing_keys: HashSet<String> =
            existing_entries.iter().map(|(_, k, _)| k.clone()).collect();

        let mut updated_keys: Vec<String> = Vec::new();
        for (_, key, current_value) in &existing_entries {
            if let Some((_, new_value)) = runtime_envs.iter().find(|(k, _)| k == key) {
                if current_value != new_value {
                    let line_idx = existing_entries
                        .iter()
                        .find(|(_, k, _)| k == key)
                        .map(|(idx, _, _)| *idx)
                        .unwrap();
                    let rendered = format!(
                        "{}{}: {}",
                        " ".repeat(entry_indent),
                        key,
                        yaml_single_quote(new_value)
                    );
                    lines[line_idx] = rendered;
                    updated_keys.push(key.clone());
                }
            }
        }

        let missing_envs = missing_runtime_envs(runtime_envs, &existing_keys);

        if updated_keys.is_empty() && missing_envs.is_empty() {
            ComposeEnvSync {
                content: compose.to_string(),
                inserted_keys: Vec::new(),
                updated_keys: Vec::new(),
            }
        } else {
            let mut inserted_keys: Vec<String> = Vec::new();
            if !missing_envs.is_empty() {
                let insert_at = env_end;
                let new_keys = missing_envs
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
                inserted_keys.extend(new_keys);
            }

            ComposeEnvSync {
                content: rebuild_compose_text(&lines, had_trailing_newline),
                inserted_keys,
                updated_keys,
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
            updated_keys: Vec::new(),
        }
    };

    Ok(sync)
}

fn ensure_service_volume_entry(
    compose: &str,
    service_name: &str,
    volume_entry: &str,
) -> std::result::Result<ComposeVolumeSync, CoolifyError> {
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

    let volumes_idx =
        (service_idx + 1..service_end).find(|index| lines[*index].trim() == "volumes:");
    let desired_target = compose_volume_target(volume_entry);

    if let Some(volumes_idx) = volumes_idx {
        let volumes_indent = leading_space_count(&lines[volumes_idx]);
        let volumes_end =
            find_block_end(&lines, volumes_idx + 1, volumes_indent).unwrap_or(service_end);
        let entry_indent = detect_list_entry_indent(&lines, volumes_idx + 1, volumes_end)
            .unwrap_or(volumes_indent + 4);
        for line in lines.iter_mut().take(volumes_end).skip(volumes_idx + 1) {
            if parse_compose_volume_entry(line).as_deref() == Some(volume_entry) {
                return Ok(ComposeVolumeSync {
                    content: compose.to_string(),
                    changed: false,
                });
            }
            if desired_target
                .as_deref()
                .is_some_and(|target| compose_volume_target_from_line(line).as_deref() == Some(target))
            {
                *line = format!("{}- '{}'", " ".repeat(entry_indent), volume_entry);
                return Ok(ComposeVolumeSync {
                    content: rebuild_compose_text(&lines, had_trailing_newline),
                    changed: true,
                });
            }
        }
        lines.insert(
            volumes_end,
            format!("{}- '{}'", " ".repeat(entry_indent), volume_entry),
        );
    } else {
        let volumes_indent = service_indent + 4;
        let entry_indent = volumes_indent + 4;
        lines.splice(
            service_end..service_end,
            [
                format!("{}volumes:", " ".repeat(volumes_indent)),
                format!("{}- '{}'", " ".repeat(entry_indent), volume_entry),
            ],
        );
    }

    Ok(ComposeVolumeSync {
        content: rebuild_compose_text(&lines, had_trailing_newline),
        changed: true,
    })
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

fn detect_list_entry_indent(
    lines: &[String],
    start_index: usize,
    end_index: usize,
) -> Option<usize> {
    (start_index..end_index).find_map(|index| {
        let line = &lines[index];
        line.trim_start()
            .starts_with("- ")
            .then(|| leading_space_count(line))
    })
}

fn parse_compose_volume_entry(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let value = trimmed.strip_prefix("- ")?.trim();
    Some(
        value
            .trim_matches('"')
            .trim_matches('\'')
            .to_string(),
    )
}

fn compose_volume_target_from_line(line: &str) -> Option<String> {
    compose_volume_target(&parse_compose_volume_entry(line)?)
}

fn compose_volume_target(volume_entry: &str) -> Option<String> {
    let mut parts = volume_entry.split(':');
    let _source = parts.next()?;
    let target = parts.next()?.trim();
    (!target.is_empty()).then(|| target.to_string())
}

fn parse_environment_entry(line: &str, env_indent: usize) -> Option<(String, String)> {
    if line.trim().is_empty() {
        return None;
    }

    let indent = leading_space_count(line);
    if indent <= env_indent {
        return None;
    }

    let trimmed = line.trim();
    let (key, value) = trimmed.split_once(':')?;
    let key = key.trim().to_string();
    let value_raw = value.trim();
    let value_unquoted = if (value_raw.starts_with('\'') && value_raw.ends_with('\''))
        || (value_raw.starts_with('"') && value_raw.ends_with('"'))
    {
        &value_raw[1..value_raw.len() - 1]
    } else {
        value_raw
    };

    Some((key, value_unquoted.replace("''", "'")))
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
    fn upsert_service_environment_entries_updates_changed_value() {
        let compose = r#"services:
    app:
        image: demo
        environment:
            COOLIFY_VPS1_BASE_URL: 'http://66.94.100.241:8000'
"#;

        let sync = upsert_service_environment_entries(
            compose,
            "app",
            &[(
                "COOLIFY_VPS1_BASE_URL".to_string(),
                "http://coolify:8080".to_string(),
            )],
        )
        .expect("compose env sync should succeed");

        assert!(sync.inserted_keys.is_empty());
        assert_eq!(sync.updated_keys, vec!["COOLIFY_VPS1_BASE_URL".to_string()]);
        assert!(sync.content.contains("http://coolify:8080"));
        assert!(!sync.content.contains("66.94.100.241"));
    }

    #[test]
    fn ensure_service_volume_entry_adds_missing_volume() {
        let compose = r#"services:
    app:
        image: demo
        volumes:
            - 'app_data:/app/data'
        environment:
            HOST: '0.0.0.0'
"#;

        let sync =
            ensure_service_volume_entry(compose, "app", "/root/studio-ssh:/home/appuser/.ssh")
                .expect("volume sync should succeed");

        assert!(sync.changed);
        assert!(sync
            .content
            .contains("- '/root/studio-ssh:/home/appuser/.ssh'"));
        assert!(sync.content.contains("environment:"));
    }

    #[test]
    fn ensure_service_volume_entry_does_not_duplicate_existing_volume() {
        let compose = r#"services:
    app:
        image: demo
        volumes:
            - '/root/studio-ssh:/home/appuser/.ssh'
"#;

        let sync =
            ensure_service_volume_entry(compose, "app", "/root/studio-ssh:/home/appuser/.ssh")
                .expect("volume sync should succeed");

        assert!(!sync.changed);
        assert_eq!(sync.content, compose);
    }

    #[test]
    fn ensure_service_volume_entry_replaces_existing_target() {
        let compose = r#"services:
    app:
        image: demo
        volumes:
            - '/root/studio-ssh:/home/appuser/.ssh:ro'
"#;

        let sync = ensure_service_volume_entry(compose, "app", "/root/studio-ssh:/home/appuser/.ssh")
            .expect("volume sync should succeed");

        assert!(sync.changed);
        assert!(sync
            .content
            .contains("- '/root/studio-ssh:/home/appuser/.ssh'"));
        assert!(!sync.content.contains(":/home/appuser/.ssh:ro"));
    }

    #[test]
    fn yaml_single_quote_escapes_single_quotes() {
        assert_eq!(yaml_single_quote("it'works"), "'it''works'");
    }
}
