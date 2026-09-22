/* Upsert de entradas environment en el compose (119A-5 split volume_manager). */

use super::texto::{
    detect_environment_entry_indent, find_block_end, leading_space_count, missing_runtime_envs,
    parse_environment_entry, rebuild_compose_text, yaml_single_quote,
};
use super::tipos::ComposeEnvSync;
use crate::error::CoolifyError;
use std::collections::HashSet;

pub(super) fn upsert_service_environment_entries(
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
        aplicar_en_bloque_existente(
            compose,
            &mut lines,
            environment_idx,
            env_indent,
            env_end,
            entry_indent,
            runtime_envs,
            had_trailing_newline,
        )
    } else {
        crear_bloque_environment(
            &mut lines,
            service_indent,
            service_end,
            runtime_envs,
            had_trailing_newline,
        )
    };

    Ok(sync)
}

/* Crea el bloque environment completo cuando el servicio no lo tiene. */
fn crear_bloque_environment(
    lines: &mut Vec<String>,
    service_indent: usize,
    service_end: usize,
    runtime_envs: &[(String, String)],
    had_trailing_newline: bool,
) -> ComposeEnvSync {
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
        content: rebuild_compose_text(lines, had_trailing_newline),
        inserted_keys,
        updated_keys: Vec::new(),
    }
}
fn aplicar_en_bloque_existente(
    compose: &str,
    lines: &mut Vec<String>,
    environment_idx: usize,
    env_indent: usize,
    env_end: usize,
    entry_indent: usize,
    runtime_envs: &[(String, String)],
    had_trailing_newline: bool,
) -> ComposeEnvSync {
        let existing_entries: Vec<(usize, String, String)> = (environment_idx + 1..env_end)
            .filter_map(|index| {
                parse_environment_entry(&lines[index], env_indent).map(|(k, v)| (index, k, v))
            })
            .collect();
        let existing_keys: HashSet<String> =
            existing_entries.iter().map(|(_, k, _)| k.clone()).collect();

        let mut updated_keys: Vec<String> = Vec::new();
        for (line_idx, key, current_value) in &existing_entries {
            if let Some((_, new_value)) = runtime_envs.iter().find(|(k, _)| k == key) {
                if current_value != new_value {
                    let rendered = format!(
                        "{}{}: {}",
                        " ".repeat(entry_indent),
                        key,
                        yaml_single_quote(new_value)
                    );
                    lines[*line_idx] = rendered;
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
}
