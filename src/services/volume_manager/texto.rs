/* Ayudantes de texto YAML para el sync de compose (119A-5 split volume_manager). */

use std::collections::HashSet;

pub(super) fn mount_points_app_uploads_to(line: &str, host_path: &str) -> bool {
    let mut parts = line.split('|').map(str::trim);
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some("/app/uploads"), Some("bind"), Some(source)) if source == host_path
    )
}

pub(super) fn leading_space_count(line: &str) -> usize {
    line.chars()
        .take_while(|character| *character == ' ')
        .count()
}

pub(super) fn find_block_end(
    lines: &[String],
    start_index: usize,
    parent_indent: usize,
) -> Option<usize> {
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

pub(super) fn detect_environment_entry_indent(
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

pub(super) fn detect_list_entry_indent(
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

pub(super) fn parse_compose_volume_entry(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let value = trimmed.strip_prefix("- ")?.trim();
    Some(value.trim_matches('"').trim_matches('\'').to_string())
}

pub(super) fn compose_volume_target_from_line(line: &str) -> Option<String> {
    compose_volume_target(&parse_compose_volume_entry(line)?)
}

pub(super) fn compose_volume_target(volume_entry: &str) -> Option<String> {
    let mut parts = volume_entry.split(':');
    let _source = parts.next()?;
    let target = parts.next()?.trim();
    (!target.is_empty()).then(|| target.to_string())
}

pub(super) fn parse_environment_entry(line: &str, env_indent: usize) -> Option<(String, String)> {
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

pub(super) fn missing_runtime_envs(
    runtime_envs: &[(String, String)],
    existing_keys: &HashSet<String>,
) -> Vec<(String, String)> {
    runtime_envs
        .iter()
        .filter(|(key, _)| !existing_keys.contains(key))
        .cloned()
        .collect()
}

pub(super) fn yaml_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub(super) fn rebuild_compose_text(lines: &[String], had_trailing_newline: bool) -> String {
    let mut content = lines.join("\n");
    if had_trailing_newline {
        content.push('\n');
    }
    content
}
