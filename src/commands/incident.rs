/*
 * [257B-1] Comandos de investigación de incidentes.
 * Proporcionan herramientas para diagnosticar freezes, Bad Gateway,
 * crashes, OOM y otros problemas en producción.
 */

use crate::commands::container::{self, ContainerEvent, ContainerInspectData};
use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::pg_utils;
use crate::infra::secrets;
use crate::infra::ssh_client::SshClient;
use crate::services::health_manager;

use serde::Serialize;
use std::time::Instant;

// ──────────────────────────────────────────────────────────────────────
// Patrones predefinidos de incidente
// ──────────────────────────────────────────────────────────────────────

const INCIDENT_PATTERNS: &[&str] = &[
    "RUNTIME FREEZE DETECTED",
    "panic",
    "OOMKilled",
    "oom-kill",
    "Out of memory",
    "no unique or exclusion constraint",
    "current transaction is aborted",
    "pool timeout",
    "connection pool",
    "too many connections",
    "response cycle",
    "outbox",
    "FATAL",
    "watchdog",
    "freeze_after",
    "SIGKILL",
    "SIGTERM",
    "Bad Gateway",
    "502",
];

// ──────────────────────────────────────────────────────────────────────
// incident-logs
// ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub timestamp: Option<String>,
    pub line: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogMatchGroup {
    pub pattern: String,
    pub matches: Vec<LogLine>,
    pub total_count: usize,
}

/// Busca logs del contenedor usando patrones predefinidos de incidente.
pub async fn incident_logs(
    _settings: &Settings,
    ssh: &SshClient,
    container_id: &str,
    since: &str,
    until: Option<&str>,
    custom_patterns: Option<Vec<String>>,
    json_output: bool,
) -> Result<Vec<LogMatchGroup>, CoolifyError> {
    let since_abs = container::resolve_relative_time(since);
    let until_flag = match until {
        Some(u) => format!(" --until '{}'", container::resolve_relative_time(u)),
        None => String::new(),
    };

    let cmd = format!(
        "docker logs --since '{}' {until_flag} {container_id} 2>&1 || true",
        since_abs
    );
    let result = ssh.execute(&cmd).await?;
    let log_text = &result.stdout;

    let mut patterns: Vec<String> = INCIDENT_PATTERNS.iter().map(|s| s.to_string()).collect();
    if let Some(custom) = custom_patterns {
        patterns.extend(custom);
    }

    let mut groups = Vec::new();
    for pattern in &patterns {
        let matches: Vec<LogLine> = log_text
            .lines()
            .filter(|line| line.contains(pattern.as_str()))
            .take(200)
            .map(|line| LogLine {
                timestamp: extract_timestamp(line),
                line: secrets::redact_text(line),
            })
            .collect();

        if !matches.is_empty() {
            let total_count = log_text
                .lines()
                .filter(|l| l.contains(pattern.as_str()))
                .count();
            groups.push(LogMatchGroup {
                pattern: pattern.clone(),
                total_count,
                matches,
            });
        }
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&groups).unwrap_or_default()
        );
    } else {
        print_incident_logs_human(&groups);
    }

    Ok(groups)
}

fn extract_timestamp(line: &str) -> Option<String> {
    if line.len() >= 20 && line.starts_with("20") {
        let ts_end = line[20..]
            .find(['Z', '+', '-', ' '])
            .map(|p| 20 + p + 1)
            .unwrap_or(20);
        Some(line[..ts_end.min(line.len())].to_string())
    } else {
        None
    }
}

fn print_incident_logs_human(groups: &[LogMatchGroup]) {
    if groups.is_empty() {
        println!("No se encontraron coincidencias de patrones de incidente.");
        return;
    }
    let total: usize = groups.iter().map(|g| g.total_count).sum();
    println!(
        "Se encontraron {} coincidencias en {} patrones:\n",
        total,
        groups.len()
    );
    for group in groups {
        println!(
            "── Patrón: '{}' ({} coincidencias) ──",
            group.pattern, group.total_count
        );
        for m in group.matches.iter().take(10) {
            let ts = m.timestamp.as_deref().unwrap_or("");
            let line_display = if m.line.len() > 200 {
                &m.line[..200]
            } else {
                &m.line
            };
            println!("  {} {}", ts, line_display);
        }
        if group.matches.len() > 10 {
            println!("  ... y {} más", group.matches.len() - 10);
        }
        println!();
    }
}

// ──────────────────────────────────────────────────────────────────────
// incident-investigate
// ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct IncidentReport {
    pub site_name: String,
    pub timestamp: String,
    pub investigation_duration_ms: u64,
    pub deployed_commit: Option<String>,
    pub container: Option<ContainerInspectData>,
    pub events: Vec<ContainerEvent>,
    pub log_matches: Vec<LogMatchGroup>,
    pub health: Option<IncidentHealthSummary>,
    pub db_stats: Option<IncidentDbSummary>,
    pub errors: Vec<SubtaskError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IncidentHealthSummary {
    pub http_ok: bool,
    pub status_code: Option<u16>,
    pub app_ok: bool,
    pub fatal_log_detected: bool,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IncidentDbSummary {
    pub active_connections: Option<i64>,
    pub long_running_queries: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubtaskError {
    pub task_name: String,
    pub error: String,
    pub duration_ms: u64,
}

/// Ejecuta una investigación completa de incidente.
pub async fn incident_investigate(
    settings: &Settings,
    site_name: &str,
    save_path: Option<&str>,
    json_output: bool,
) -> Result<IncidentReport, CoolifyError> {
    let start = Instant::now();
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
    let target_config = settings.resolve_site_target(site)?;
    let mut ssh = SshClient::from_vps(&target_config.vps);
    ssh.connect().await?;

    let app_container = match caps.resolve_app_container(&ssh, stack_uuid).await {
        Ok(c) => c,
        Err(e) => {
            let report = IncidentReport {
                site_name: site_name.to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                investigation_duration_ms: start.elapsed().as_millis() as u64,
                deployed_commit: None,
                container: None,
                events: Vec::new(),
                log_matches: Vec::new(),
                health: None,
                db_stats: None,
                errors: vec![SubtaskError {
                    task_name: "resolve_container".into(),
                    error: e.to_string(),
                    duration_ms: start.elapsed().as_millis() as u64,
                }],
            };
            return emit_report(&report, json_output, save_path);
        }
    };

    let mut errors: Vec<SubtaskError> = Vec::new();

    /* 1. Commit desplegado */
    let t = Instant::now();
    let deployed_commit = match get_deployed_commit(&ssh, &app_container).await {
        Ok(c) => Some(c),
        Err(e) => {
            errors.push(SubtaskError {
                task_name: "deployed_commit".into(),
                error: e.to_string(),
                duration_ms: t.elapsed().as_millis() as u64,
            });
            None
        }
    };

    /* 2. Container inspect */
    let t = Instant::now();
    let container_data = match container::inspect_container(
        settings,
        site_name,
        &ssh,
        &app_container,
        false,
    )
    .await
    {
        Ok(d) => Some(d),
        Err(e) => {
            errors.push(SubtaskError {
                task_name: "container_inspect".into(),
                error: e.to_string(),
                duration_ms: t.elapsed().as_millis() as u64,
            });
            None
        }
    };

    /* 3. Container events (últimas 48h) */
    let t = Instant::now();
    let events =
        match container::container_events(settings, &ssh, &app_container, "48h", None, false).await
        {
            Ok(e) => e,
            Err(e) => {
                errors.push(SubtaskError {
                    task_name: "container_events".into(),
                    error: e.to_string(),
                    duration_ms: t.elapsed().as_millis() as u64,
                });
                Vec::new()
            }
        };

    /* 4. Incident logs */
    let t = Instant::now();
    let log_matches =
        match incident_logs(settings, &ssh, &app_container, "48h", None, None, false).await {
            Ok(l) => l,
            Err(e) => {
                errors.push(SubtaskError {
                    task_name: "incident_logs".into(),
                    error: e.to_string(),
                    duration_ms: t.elapsed().as_millis() as u64,
                });
                Vec::new()
            }
        };

    /* 5. Health check */
    let t = Instant::now();
    let health = match health_manager::run_site_health_check(settings, site, &ssh).await {
        Ok(report) => Some(IncidentHealthSummary {
            http_ok: report.http_ok,
            status_code: report.status_code,
            app_ok: report.app_ok,
            fatal_log_detected: report.fatal_log_detected,
            details: report.details,
        }),
        Err(e) => {
            errors.push(SubtaskError {
                task_name: "health_check".into(),
                error: e.to_string(),
                duration_ms: t.elapsed().as_millis() as u64,
            });
            None
        }
    };

    /* 6. DB stats (rápida) */
    let t = Instant::now();
    let db_stats = match get_quick_db_stats(&ssh, stack_uuid).await {
        Ok(s) => Some(s),
        Err(e) => {
            errors.push(SubtaskError {
                task_name: "db_stats".into(),
                error: e.to_string(),
                duration_ms: t.elapsed().as_millis() as u64,
            });
            None
        }
    };

    let report = IncidentReport {
        site_name: site_name.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        investigation_duration_ms: start.elapsed().as_millis() as u64,
        deployed_commit,
        container: container_data,
        events,
        log_matches,
        health,
        db_stats,
        errors,
    };

    emit_report(&report, json_output, save_path)
}

async fn get_deployed_commit(ssh: &SshClient, container_id: &str) -> Result<String, CoolifyError> {
    let result = docker::docker_exec(
        ssh,
        container_id,
        "git rev-parse HEAD 2>/dev/null || echo unknown",
    )
    .await?;
    let commit = result.stdout.trim().to_string();
    if commit.is_empty() || commit == "unknown" {
        let label = docker::docker_exec(
            ssh,
            container_id,
            "cat /app/VERSION 2>/dev/null || echo unknown",
        )
        .await?;
        Ok(label.stdout.trim().to_string())
    } else {
        Ok(commit)
    }
}

async fn get_quick_db_stats(
    ssh: &SshClient,
    stack_uuid: &str,
) -> Result<IncidentDbSummary, CoolifyError> {
    let (pg_container, db_user, db_name, _) = pg_utils::get_pg_credentials(ssh, stack_uuid).await?;

    let active_sql = "SELECT count(*) FROM pg_stat_activity WHERE state = 'active';";
    let active = pg_utils::run_pg_query(ssh, &pg_container, &db_user, &db_name, active_sql)
        .await
        .ok()
        .and_then(|s| s.trim().parse().ok());

    let long_sql = "SELECT pid, now() - query_start AS duration, left(query, 100) \
         FROM pg_stat_activity \
         WHERE state = 'active' AND now() - query_start > interval '5 seconds' \
         ORDER BY duration DESC LIMIT 10;";
    let long_running_raw = pg_utils::run_pg_query(ssh, &pg_container, &db_user, &db_name, long_sql)
        .await
        .unwrap_or_default();

    let long_running_queries: Vec<String> = long_running_raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(secrets::redact_text)
        .collect();

    Ok(IncidentDbSummary {
        active_connections: active,
        long_running_queries,
        error: None,
    })
}

fn emit_report(
    report: &IncidentReport,
    json_output: bool,
    save_path: Option<&str>,
) -> Result<IncidentReport, CoolifyError> {
    if json_output {
        let json = serde_json::to_string_pretty(report).unwrap_or_default();
        println!("{}", json);
        if let Some(path) = save_path {
            std::fs::write(path, &json)?;
            eprintln!("Reporte guardado en: {}", path);
        }
    } else {
        print_investigate_human(report);
        if let Some(path) = save_path {
            let json = serde_json::to_string_pretty(report).unwrap_or_default();
            std::fs::write(path, &json)?;
            eprintln!("Reporte guardado en: {}", path);
        }
    }
    Ok(report.clone())
}

fn print_investigate_human(report: &IncidentReport) {
    println!("═══════════════════════════════════════════════════");
    println!("  INCIDENT INVESTIGATE: {}", report.site_name);
    println!("  {}", report.timestamp);
    println!("  Duración: {}ms", report.investigation_duration_ms);
    println!("═══════════════════════════════════════════════════\n");

    if let Some(ref commit) = report.deployed_commit {
        println!("Commit desplegado: {}", commit);
    }

    if let Some(ref c) = report.container {
        println!("\n── Contenedor ──");
        println!("  ID:            {}", c.container_id);
        println!("  Estado:        {}", c.state);
        println!(
            "  Inicio:        {}",
            c.start_time.as_deref().unwrap_or("?")
        );
        println!("  Restart count: {}", c.restart_count);
        println!("  OOM killed:    {}", c.oom_killed);
        if let Some(ec) = c.exit_code {
            println!("  Exit code:     {}", ec);
        }
        if let Some(ref err) = c.error_message {
            println!("  Error:         {}", err);
        }
    }

    if !report.events.is_empty() {
        println!("\n── Eventos (últimas 48h): {} ──", report.events.len());
        for e in &report.events {
            println!(
                "  {} {} exit={}",
                e.timestamp,
                e.action,
                e.exit_code.map_or("-".into(), |c| c.to_string())
            );
        }
    }

    if !report.log_matches.is_empty() {
        println!("\n── Logs con patrones de incidente ──");
        for group in &report.log_matches {
            println!(
                "  '{}' → {} coincidencias",
                group.pattern, group.total_count
            );
        }
    }

    if let Some(ref h) = report.health {
        println!("\n── Health ──");
        println!("  HTTP OK:       {}", h.http_ok);
        if let Some(code) = h.status_code {
            println!("  Status code:   {}", code);
        }
        println!("  App OK:        {}", h.app_ok);
        println!("  Fatal logs:    {}", h.fatal_log_detected);
    }

    if let Some(ref db) = report.db_stats {
        println!("\n── DB Stats ──");
        if let Some(conn) = db.active_connections {
            println!("  Conexiones activas: {}", conn);
        }
        if !db.long_running_queries.is_empty() {
            println!("  Queries >5s:");
            for q in &db.long_running_queries {
                println!("    {}", q);
            }
        }
    }

    if !report.errors.is_empty() {
        println!("\n── Errores parciales ──");
        for e in &report.errors {
            println!("  [{}] {} ({}ms)", e.task_name, e.error, e.duration_ms);
        }
    }

    println!("\n═══════════════════════════════════════════════════");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_incident_patterns_not_empty() {
        assert!(!INCIDENT_PATTERNS.is_empty());
        assert!(INCIDENT_PATTERNS.contains(&"RUNTIME FREEZE DETECTED"));
        assert!(INCIDENT_PATTERNS.contains(&"panic"));
        assert!(INCIDENT_PATTERNS.contains(&"OOMKilled"));
        assert!(INCIDENT_PATTERNS.contains(&"no unique or exclusion constraint"));
        assert!(INCIDENT_PATTERNS.contains(&"current transaction is aborted"));
    }

    #[test]
    fn test_extract_timestamp() {
        let line = "2026-07-24T11:06:29Z something happened";
        let ts = extract_timestamp(line);
        assert!(ts.is_some());
        assert!(ts.unwrap().starts_with("2026-07-24T11:06:29"));
        assert_eq!(extract_timestamp("no timestamp here"), None);
    }
}
