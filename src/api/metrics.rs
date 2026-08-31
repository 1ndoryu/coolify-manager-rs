/*
 * Metricas de despliegues: construccion del script SSH de `docker stats`
 * y parsing de su salida tabulada.
 * Extraido de api/mod.rs (Fase H) para mantener la capa API publica por
 * debajo del limite de lineas (regla limite-lineas); es un concern unico
 * con entrada (sitios+uuid) y salida (Vec<DeploymentMetric>) bien acotadas.
 */

use crate::api::types::{ContainerMetric, DeploymentMetric};
use std::collections::HashMap;

use super::shell_quote;

#[derive(Debug, Clone)]
pub struct MetricQuerySite {
    pub name: String,
    pub stack_uuid: String,
}

pub fn build_metrics_script(sites: &[MetricQuerySite]) -> String {
    let mut script = String::from("set -o pipefail; ");
    for site in sites {
        script.push_str(&format!(
            "site={}; uuid={}; ids=$(docker ps --filter \"name=$uuid\" -q | tr '\\n' ' '); if [ -z \"$ids\" ]; then printf '%s\\t\\t0%\\t0B / 0B\\t0%\\tstopped\\n' \"$site\"; else docker stats --no-stream --format \"$site\\t{{{{.Name}}}}\\t{{{{.CPUPerc}}}}\\t{{{{.MemUsage}}}}\\t{{{{.MemPerc}}}}\\trunning\" $ids; fi; ",
            shell_quote(&site.name),
            shell_quote(&site.stack_uuid)
        ));
    }

    format!("bash -lc {}", shell_quote(&script))
}

pub fn parse_metrics_output(
    target: &str,
    generated_at: &str,
    sites: &[MetricQuerySite],
    output: &str,
) -> Vec<DeploymentMetric> {
    let mut by_site: HashMap<String, DeploymentMetric> = sites
        .iter()
        .map(|site| {
            (
                site.name.clone(),
                DeploymentMetric {
                    site_name: site.name.clone(),
                    target: target.to_string(),
                    status: "sin-contenedores".to_string(),
                    total_cpu_percent: 0.0,
                    memory_used_bytes: 0,
                    memory_limit_bytes: 0,
                    memory_percent: 0.0,
                    containers: Vec::new(),
                    updated_at: generated_at.to_string(),
                },
            )
        })
        .collect();

    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let columns: Vec<&str> = line.split('\t').collect();
        if columns.len() < 6 {
            continue;
        }

        let site_name = columns[0].to_string();
        let container_name = columns[1].to_string();
        let status = columns[5].to_string();
        let entry = by_site
            .entry(site_name.clone())
            .or_insert(DeploymentMetric {
                site_name: site_name.clone(),
                target: target.to_string(),
                status: status.clone(),
                total_cpu_percent: 0.0,
                memory_used_bytes: 0,
                memory_limit_bytes: 0,
                memory_percent: 0.0,
                containers: Vec::new(),
                updated_at: generated_at.to_string(),
            });

        entry.status = status.clone();
        if status != "running" || container_name.is_empty() {
            continue;
        }

        let cpu_percent = parse_percent(columns[2]);
        let (memory_used_bytes, memory_limit_bytes) = parse_memory_usage(columns[3]);
        let memory_percent = parse_percent(columns[4]);
        entry.total_cpu_percent += cpu_percent;
        entry.memory_used_bytes += memory_used_bytes;
        entry.memory_limit_bytes += memory_limit_bytes;
        entry.containers.push(ContainerMetric {
            name: container_name,
            cpu_percent,
            memory_usage: columns[3].to_string(),
            memory_percent,
            memory_used_bytes,
            memory_limit_bytes,
        });
    }

    let mut metrics: Vec<DeploymentMetric> = by_site
        .into_values()
        .map(|mut metric| {
            if metric.memory_limit_bytes > 0 {
                metric.memory_percent =
                    (metric.memory_used_bytes as f32 / metric.memory_limit_bytes as f32) * 100.0;
            }
            metric
        })
        .collect();
    metrics.sort_by(|a, b| a.site_name.cmp(&b.site_name));
    metrics
}

pub fn parse_percent(value: &str) -> f32 {
    value
        .trim()
        .trim_end_matches('%')
        .parse::<f32>()
        .unwrap_or(0.0)
}

pub fn parse_memory_usage(value: &str) -> (u64, u64) {
    let mut parts = value.split('/').map(str::trim);
    let used = parts.next().map(parse_memory_value).unwrap_or(0);
    let limit = parts.next().map(parse_memory_value).unwrap_or(0);
    (used, limit)
}

pub fn parse_memory_value(value: &str) -> u64 {
    let trimmed = value.trim();
    let number: String = trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect();
    let unit = trimmed[number.len()..].trim().to_ascii_lowercase();
    let base = number.parse::<f64>().unwrap_or(0.0);
    let multiplier = match unit.as_str() {
        "gib" | "gb" | "g" => 1024_f64.powi(3),
        "mib" | "mb" | "m" => 1024_f64.powi(2),
        "kib" | "kb" | "k" => 1024_f64,
        _ => 1.0,
    };
    (base * multiplier) as u64
}
