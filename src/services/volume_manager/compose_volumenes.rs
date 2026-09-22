/* Entrada volumes en el compose (119A-5 split volume_manager). */

use super::texto::{
    compose_volume_target, compose_volume_target_from_line, detect_list_entry_indent,
    find_block_end, leading_space_count, parse_compose_volume_entry, rebuild_compose_text,
};
use super::tipos::ComposeVolumeSync;
use crate::error::CoolifyError;

pub(super) fn ensure_service_volume_entry(
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
            if desired_target.as_deref().is_some_and(|target| {
                compose_volume_target_from_line(line).as_deref() == Some(target)
            }) {
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
