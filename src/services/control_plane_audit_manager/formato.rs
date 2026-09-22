/* Formato y parseo de salidas del auditor (119A-5 split control_plane_audit_manager). */

use super::tipos::{ContainerStat, CONTROL_PLANE_CONTAINERS, COOLIFY_PROXY_CONTAINER};
use std::collections::BTreeSet;

pub(super) fn is_control_plane_container(name: &str, include_proxy: bool) -> bool {
    CONTROL_PLANE_CONTAINERS
        .iter()
        .any(|candidate| candidate == &name)
        || (include_proxy && name == COOLIFY_PROXY_CONTAINER)
}

pub(super) fn parse_name_set(raw: &str) -> BTreeSet<String> {
    raw.lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| *value != "null")
        .map(ToString::to_string)
        .collect()
}

pub(super) fn should_sync_proxy_network(network: &str) -> bool {
    !matches!(network, "bridge" | "host" | "none" | "ingress") && !network.ends_with("_ssh_net")
}

pub(super) fn format_name_list(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(",")
    }
}

pub(super) fn parse_container_stats(raw: &str) -> Vec<ContainerStat> {
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.split('|');
            let name = parts.next()?.trim();
            let cpu = parts.next()?.trim().trim_end_matches('%');
            let mem_usage = parts.next()?.trim();
            let block_io = parts.next()?.trim();
            Some(ContainerStat {
                name: name.to_string(),
                cpu_percent: cpu.parse::<f32>().unwrap_or(0.0),
                mem_usage: mem_usage.to_string(),
                block_io: block_io.to_string(),
            })
        })
        .collect()
}

pub(super) fn build_dominance_summary(stats: &[ContainerStat]) -> String {
    let Some(first) = stats.first() else {
        return "sin-datos".to_string();
    };
    let second_cpu = stats.get(1).map(|stat| stat.cpu_percent).unwrap_or(0.0);
    let total_cpu: f32 = stats.iter().map(|stat| stat.cpu_percent).sum();
    let dominant = first.name == "coolify"
        && first.cpu_percent >= 50.0
        && (second_cpu == 0.0 || first.cpu_percent >= second_cpu * 3.0);

    if dominant {
        format!(
            "coolify domina el control-plane ({:.2}% CPU; siguiente {:.2}% CPU; total control-plane {:.2}% CPU)",
            first.cpu_percent, second_cpu, total_cpu
        )
    } else {
        format!(
            "sin dominancia extrema (hotspot={} {:.2}% CPU; total control-plane {:.2}% CPU)",
            first.name, first.cpu_percent, total_cpu
        )
    }
}

pub(super) fn empty_as_unknown(value: &str) -> String {
    if value.trim().is_empty() {
        "unknown".to_string()
    } else {
        value.trim().to_string()
    }
}

pub(super) fn empty_as_unknown_multiline(value: &str) -> String {
    if value.trim().is_empty() {
        "unknown".to_string()
    } else {
        value.trim().replace('\r', "")
    }
}

pub(super) fn ok_if_empty(value: &str) -> String {
    if value.trim().is_empty() {
        "ok".to_string()
    } else {
        value.trim().replace('\n', " ").replace('\r', "")
    }
}

pub(super) fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
