/*
 * [257B-1] Comandos de inspección de contenedores Docker.
 * Proporcionan información detallada del estado, eventos y recursos
 * de contenedores remotos via SSH, sin mutar nada.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use serde::Serialize;
use std::collections::HashMap;

// ──────────────────────────────────────────────────────────────────────
// container-inspect
// ──────────────────────────────────────────────────────────────────────

/// Datos extraídos de `docker inspect` para un contenedor.
#[derive(Debug, Clone, Serialize)]
pub struct ContainerInspectData {
    pub container_id: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub start_time: Option<String>,
    pub restart_count: i64,
    pub oom_killed: bool,
    pub exit_code: Option<i64>,
    pub error_message: Option<String>,
    pub memory_limit_mb: Option<i64>,
    pub cpu_shares: Option<i64>,
    pub restart_policy: String,
}

/// Ejecuta `docker inspect` sobre el contenedor app de un sitio y parsea los campos
/// relevantes. No imprime env vars ni secretos.
pub async fn inspect_container(
    _settings: &Settings,
    site_name: &str,
    ssh: &SshClient,
    container_id: &str,
    json_output: bool,
) -> Result<ContainerInspectData, CoolifyError> {
    let format_str = "{{.Id}}\\t{{.Config.Image}}\\t{{.State.Status}}\\t{{.State.Running}}\\t{{.State.StartedAt}}\\t{{.RestartCount}}\\t{{.State.OOMKilled}}\\t{{.State.ExitCode}}\\t{{.State.Error}}\\t{{.HostConfig.Memory}}\\t{{.HostConfig.CpuShares}}\\t{{.HostConfig.RestartPolicy.Name}}";
    let cmd = format!(
        "docker inspect --format '{}' {} 2>/dev/null",
        format_str, container_id
    );
    let result = ssh.execute(&cmd).await?;

    if !result.success() || result.stdout.trim().is_empty() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo inspeccionar contenedor '{}' para sitio '{}'",
            container_id, site_name
        )));
    }

    let parts: Vec<&str> = result.stdout.trim().split('\t').collect();
    if parts.len() < 12 {
        return Err(CoolifyError::Validation(format!(
            "Salida de docker inspect inesperada para '{}': campos insuficientes",
            container_id
        )));
    }

    let full_id = parts[0].to_string();
    let data = ContainerInspectData {
        container_id: if full_id.len() > 12 {
            full_id[..12].to_string()
        } else {
            full_id
        },
        image: parts[1].to_string(),
        state: parts[2].to_string(),
        status: format!("running={}", parts[3]),
        start_time: if parts[4].is_empty() {
            None
        } else {
            Some(parts[4].to_string())
        },
        restart_count: parts[5].parse().unwrap_or(0),
        oom_killed: parts[6].parse().unwrap_or(false),
        exit_code: parts[7].parse().ok(),
        error_message: if parts[8].is_empty() {
            None
        } else {
            Some(parts[8].to_string())
        },
        memory_limit_mb: parse_memory_to_mb(parts[9]),
        cpu_shares: parts[10].parse().ok(),
        restart_policy: parts[11].to_string(),
    };

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&data).unwrap_or_default()
        );
    } else {
        print_container_inspect_human(&data);
    }

    Ok(data)
}

fn parse_memory_to_mb(raw: &str) -> Option<i64> {
    let val: i64 = raw.parse().ok()?;
    if val == 0 {
        None
    } else {
        Some(val / 1024 / 1024)
    }
}

fn print_container_inspect_human(data: &ContainerInspectData) {
    println!("Container:      {}", data.container_id);
    println!("Image:          {}", data.image);
    println!("State:          {} ({})", data.state, data.status);
    if let Some(ref t) = data.start_time {
        println!("Started:        {}", t);
    }
    println!("Restart count:  {}", data.restart_count);
    println!("OOM killed:     {}", data.oom_killed);
    if let Some(ec) = data.exit_code {
        println!("Exit code:      {}", ec);
    }
    if let Some(ref err) = data.error_message {
        println!("Error:          {}", err);
    }
    if let Some(mb) = data.memory_limit_mb {
        println!("Memory limit:   {} MB", mb);
    }
    if let Some(cs) = data.cpu_shares {
        println!("CPU shares:     {}", cs);
    }
    println!("Restart policy: {}", data.restart_policy);
}

/// Helper para obtener el container ID del app de un sitio via SSH.
pub async fn resolve_app_container_id(
    settings: &Settings,
    site_name: &str,
    ssh: &SshClient,
) -> Result<String, CoolifyError> {
    let site = settings
        .sitios
        .iter()
        .find(|s| s.nombre == site_name)
        .ok_or_else(|| CoolifyError::Validation(format!("Sitio '{}' no encontrado", site_name)))?;
    let stack_uuid = site
        .stack_uuid
        .as_deref()
        .ok_or_else(|| CoolifyError::Validation(format!("Sitio '{}' sin stackUuid", site_name)))?;
    let caps = crate::services::site_capabilities::resolve(site);
    caps.resolve_app_container(ssh, stack_uuid).await
}

// ──────────────────────────────────────────────────────────────────────
// container-events
// ──────────────────────────────────────────────────────────────────────

/// Evento de ciclo de vida de un contenedor Docker.
#[derive(Debug, Clone, Serialize)]
pub struct ContainerEvent {
    pub timestamp: String,
    pub action: String,
    pub exit_code: Option<i64>,
    pub signal: Option<String>,
    pub attributes: HashMap<String, String>,
}

/// Convierte formatos relativos como "24h", "2d" a timestamp Unix absoluto.
/// Maneja input vacío o inválido devolviendo un default de 24 horas.
pub fn resolve_relative_time(input: &str) -> String {
    let input = input.trim();
    if input.is_empty() {
        /* Default: 24 horas atrás */
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        return (now - 86400).to_string();
    }
    /* Si ya es un timestamp numérico, devolver tal cual */
    if input.chars().all(|c| c.is_ascii_digit()) {
        return input.to_string();
    }
    /* Parsear sufijo: 1h, 24h, 2d, 30m */
    /* Guard: input de 1 char no numérico (ej: "x") produce num_str="" */
    if input.len() < 2 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        return (now - 86400).to_string();
    }
    let (num_str, suffix) = input.split_at(input.len() - 1);
    /* Si num_str está vacío o no es numérico, usar 24 como default */
    let num: i64 = if num_str.is_empty() || !num_str.chars().all(|c| c.is_ascii_digit()) {
        24
    } else {
        num_str.parse().unwrap_or(24)
    };
    let seconds = match suffix {
        "m" => num * 60,
        "h" => num * 3600,
        "d" => num * 86400,
        _ => num * 3600,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    (now - seconds).to_string()
}

/// Recupera eventos de ciclo de vida de un contenedor usando `docker events`.
pub async fn container_events(
    _settings: &Settings,
    ssh: &SshClient,
    container_name: &str,
    since: &str,
    until: Option<&str>,
    json_output: bool,
) -> Result<Vec<ContainerEvent>, CoolifyError> {
    let since_abs = resolve_relative_time(since);
    let until_flag = match until {
        Some(u) => format!(" --until '{}'", resolve_relative_time(u)),
        None => String::new(),
    };

    let cmd = format!(
        "docker events --filter 'container={container_name}' --since '{}' {until_flag} --format '{{{{json .}}}}' 2>/dev/null || true",
        since_abs
    );
    let result = ssh.execute(&cmd).await?;
    let raw = result.stdout.trim();

    let mut events = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            let action = val
                .get("Action")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            if !matches!(
                action.as_str(),
                "create" | "start" | "die" | "destroy" | "oom" | "kill" | "stop" | "restart"
            ) {
                continue;
            }
            let ts = val
                .get("time")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let exit_code = val
                .get("Actor")
                .and_then(|a| a.get("Attributes"))
                .and_then(|a| a.get("exitCode"))
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok());
            let signal = val
                .get("Actor")
                .and_then(|a| a.get("Attributes"))
                .and_then(|a| a.get("signal"))
                .and_then(|v| v.as_str())
                .map(String::from);

            events.push(ContainerEvent {
                timestamp: ts,
                action,
                exit_code,
                signal,
                attributes: HashMap::new(),
            });
        }
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&events).unwrap_or_default()
        );
    } else {
        print_events_human(&events);
    }

    Ok(events)
}

fn print_events_human(events: &[ContainerEvent]) {
    if events.is_empty() {
        println!("No se encontraron eventos en el rango especificado.");
        return;
    }
    /* [257B-1] Inline string literals to satisfy clippy::print_literal */
    println!(
        "{:<22} {:<12} {:<10} SIGNAL",
        "TIMESTAMP", "ACTION", "EXIT_CODE"
    );
    println!("{}", "-".repeat(60));
    for e in events {
        println!(
            "{:<22} {:<12} {:<10} {}",
            e.timestamp,
            e.action,
            e.exit_code.map_or("-".to_string(), |c| c.to_string()),
            e.signal.as_deref().unwrap_or("-")
        );
    }
}

// ──────────────────────────────────────────────────────────────────────
// container-stats
// ──────────────────────────────────────────────────────────────────────

/// Uso de recursos de un contenedor en un punto de tiempo.
#[derive(Debug, Clone, Serialize)]
pub struct ContainerStats {
    pub container_id: String,
    pub name: String,
    pub cpu_percent: String,
    pub memory_usage: String,
    pub memory_limit: String,
    pub memory_percent: String,
    pub net_io: String,
    pub block_io: String,
    pub pids: String,
}

/// Obtiene métricas de recursos del contenedor usando `docker stats --no-stream`.
pub async fn container_stats(
    _settings: &Settings,
    ssh: &SshClient,
    container_id: &str,
    json_output: bool,
) -> Result<ContainerStats, CoolifyError> {
    let format_str = "{{.ID}}\\t{{.Name}}\\t{{.CPUPerc}}\\t{{.MemUsage}}\\t{{.MemPerc}}\\t{{.NetIO}}\\t{{.BlockIO}}\\t{{.PIDs}}";
    let cmd = format!(
        "docker stats --no-stream --format '{}' {} 2>/dev/null",
        format_str, container_id
    );
    let result = ssh.execute(&cmd).await?;

    if !result.success() || result.stdout.trim().is_empty() {
        return Err(CoolifyError::Validation(format!(
            "No se pudieron obtener stats del contenedor '{}'",
            container_id
        )));
    }

    let parts: Vec<&str> = result.stdout.trim().split('\t').collect();
    if parts.len() < 8 {
        return Err(CoolifyError::Validation(
            "Salida de docker stats inesperada".to_string(),
        ));
    }

    let mem_parts: Vec<&str> = parts[3].split(" / ").collect();

    let stats = ContainerStats {
        container_id: parts[0].to_string(),
        name: parts[1].to_string(),
        cpu_percent: parts[2].to_string(),
        memory_usage: mem_parts.first().unwrap_or(&"").to_string(),
        memory_limit: mem_parts.get(1).unwrap_or(&"").to_string(),
        memory_percent: parts[4].to_string(),
        net_io: parts[5].to_string(),
        block_io: parts[6].to_string(),
        pids: parts[7].to_string(),
    };

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&stats).unwrap_or_default()
        );
    } else {
        print_stats_human(&stats);
    }

    Ok(stats)
}

fn print_stats_human(stats: &ContainerStats) {
    println!("Container:  {} ({})", stats.name, stats.container_id);
    println!("CPU:        {}", stats.cpu_percent);
    println!(
        "Memory:     {} / {} ({})",
        stats.memory_usage, stats.memory_limit, stats.memory_percent
    );
    println!("Net I/O:    {}", stats.net_io);
    println!("Block I/O:  {}", stats.block_io);
    println!("PIDs:       {}", stats.pids);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_relative_time_hours() {
        let result = resolve_relative_time("24h");
        assert!(result.chars().all(|c| c.is_ascii_digit()));
        let ts: i64 = result.parse().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!((now - ts - 86400).abs() < 5);
    }

    #[test]
    fn test_resolve_relative_time_empty() {
        let result = resolve_relative_time("");
        assert!(result.chars().all(|c| c.is_ascii_digit()));
        let ts: i64 = result.parse().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!((now - ts - 86400).abs() < 5);
    }

    #[test]
    fn test_resolve_relative_time_passthrough() {
        assert_eq!(resolve_relative_time("1627000000"), "1627000000");
    }

    #[test]
    fn test_resolve_relative_time_single_char() {
        /* "x" (1 char, no numérico) → default 24h */
        let result = resolve_relative_time("x");
        assert!(result.chars().all(|c| c.is_ascii_digit()));
        let ts: i64 = result.parse().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!((now - ts - 86400).abs() < 5);
    }

    #[test]
    fn test_parse_memory_to_mb() {
        assert_eq!(parse_memory_to_mb("0"), None);
        assert_eq!(parse_memory_to_mb("536870912"), Some(512));
        assert_eq!(parse_memory_to_mb("invalid"), None);
    }
}
