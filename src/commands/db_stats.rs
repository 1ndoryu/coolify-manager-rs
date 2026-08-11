/*
 * [257B-1] Comando db-stats.
 * Proporciona métricas rápidas de PostgreSQL: conexiones activas,
 * queries lentas, lock waits, dead tuples y tamaño de tablas.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::pg_utils;
use crate::infra::secrets;
use crate::infra::ssh_client::SshClient;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DbStats {
    pub site_name: String,
    pub connections_by_state: Vec<ConnectionState>,
    pub long_running_queries: Vec<QueryStat>,
    pub lock_waits: Vec<LockWait>,
    pub deadlocks_total: i64,
    pub top_tables: Vec<TableStats>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionState {
    pub state: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueryStat {
    pub pid: i64,
    pub duration_secs: f64,
    pub state: String,
    pub query_preview: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LockWait {
    pub blocked_pid: i64,
    pub blocking_pid: i64,
    pub lock_type: String,
    pub query_preview: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TableStats {
    pub table_name: String,
    pub row_estimate: i64,
    pub total_size: String,
    pub dead_tuples: i64,
    pub last_vacuum: Option<String>,
    pub last_analyze: Option<String>,
}

/// Ejecuta consultas de diagnóstico contra PostgreSQL y devuelve métricas.
pub async fn execute(
    settings: &Settings,
    site_name: &str,
    threshold_secs: u32,
    json_output: bool,
) -> Result<DbStats, CoolifyError> {
    let site = settings
        .sitios
        .iter()
        .find(|s| s.nombre == site_name)
        .ok_or_else(|| CoolifyError::Validation(format!("Sitio '{}' no encontrado", site_name)))?;

    let stack_uuid = site
        .stack_uuid
        .as_deref()
        .ok_or_else(|| CoolifyError::Validation(format!("Sitio '{}' sin stackUuid", site_name)))?;

    let target_config = settings.resolve_site_target(site)?;
    let mut ssh = SshClient::from_vps(&target_config.vps);
    ssh.connect().await?;

    let (pg_container, db_user, db_name, _) =
        pg_utils::get_pg_credentials(&ssh, stack_uuid).await?;

    /* ── 1. Conexiones por estado ── */
    let conn_sql = "SELECT coalesce(state, 'unknown'), count(*) FROM pg_stat_activity GROUP BY state ORDER BY count DESC;";
    let conn_raw = pg_utils::run_pg_query(&ssh, &pg_container, &db_user, &db_name, conn_sql)
        .await
        .unwrap_or_default();
    let connections_by_state: Vec<ConnectionState> = conn_raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(2, ',').collect();
            if parts.len() == 2 {
                let state = parts[0].trim().trim_matches('\'').to_string();
                let count: i64 = parts[1].trim().parse().unwrap_or(0);
                Some(ConnectionState { state, count })
            } else {
                None
            }
        })
        .collect();

    /* ── 2. Queries largas ── */
    /* SAFETY: threshold_secs es u32 (CLI parser garantiza), no inyectable.
     * Si en el futuro se acepta input de usuario como String, usar parameterized
     * query o validar whitelist. */
    let long_sql = format!(
        "SELECT pid, EXTRACT(EPOCH FROM (now() - query_start))::int, coalesce(state,''), left(query, 120) \
         FROM pg_stat_activity \
         WHERE state = 'active' AND now() - query_start > interval '{threshold_secs} seconds' \
         AND query NOT LIKE '%pg_stat_activity%' \
         ORDER BY query_start LIMIT 15;"
    );
    let long_raw = pg_utils::run_pg_query(&ssh, &pg_container, &db_user, &db_name, &long_sql)
        .await
        .unwrap_or_default();
    let long_running_queries: Vec<QueryStat> = long_raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(4, ',').collect();
            if parts.len() >= 4 {
                Some(QueryStat {
                    pid: parts[0].trim().parse().unwrap_or(0),
                    duration_secs: parts[1].trim().parse().unwrap_or(0.0),
                    state: parts[2].trim().to_string(),
                    query_preview: secrets::redact_text(parts[3].trim()),
                })
            } else {
                None
            }
        })
        .collect();

    /* ── 3. Lock waits ── */
    let lock_sql =
        "SELECT blocked.pid, blocking.pid, blocked.mode, left(blocked_activity.query, 80) \
         FROM pg_locks blocked \
         JOIN pg_locks blocking ON blocking.locktype = blocked.locktype \
             AND blocking.database IS NOT DISTINCT FROM blocked.database \
             AND blocking.relation IS NOT DISTINCT FROM blocked.relation \
             AND blocking.pid != blocked.pid \
         JOIN pg_stat_activity blocked_activity ON blocked_activity.pid = blocked.pid \
         WHERE NOT blocked.granted \
         LIMIT 10;";
    let lock_raw = pg_utils::run_pg_query(&ssh, &pg_container, &db_user, &db_name, lock_sql)
        .await
        .unwrap_or_default();
    let lock_waits: Vec<LockWait> = lock_raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(4, ',').collect();
            if parts.len() >= 4 {
                Some(LockWait {
                    blocked_pid: parts[0].trim().parse().unwrap_or(0),
                    blocking_pid: parts[1].trim().parse().unwrap_or(0),
                    lock_type: parts[2].trim().to_string(),
                    query_preview: secrets::redact_text(parts[3].trim()),
                })
            } else {
                None
            }
        })
        .collect();

    /* ── 4. Deadlocks ── */
    let dl_sql = "SELECT deadlocks FROM pg_stat_database WHERE datname = current_database();";
    let deadlocks_total = pg_utils::run_pg_query(&ssh, &pg_container, &db_user, &db_name, dl_sql)
        .await
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);

    /* ── 5. Top tablas por tamaño ── */
    let tables_sql = "SELECT relname, n_live_tup, pg_size_pretty(pg_total_relation_size(relid)), \
         n_dead_tup, to_char(last_vacuum, 'YYYY-MM-DD HH24:MI'), to_char(last_analyze, 'YYYY-MM-DD HH24:MI') \
         FROM pg_stat_user_tables \
         ORDER BY pg_total_relation_size(relid) DESC LIMIT 15;";
    let tables_raw = pg_utils::run_pg_query(&ssh, &pg_container, &db_user, &db_name, tables_sql)
        .await
        .unwrap_or_default();
    let top_tables: Vec<TableStats> = tables_raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(6, ',').collect();
            if parts.len() >= 4 {
                Some(TableStats {
                    table_name: parts[0].trim().to_string(),
                    row_estimate: parts[1].trim().parse().unwrap_or(0),
                    total_size: parts[2].trim().to_string(),
                    dead_tuples: parts[3].trim().parse().unwrap_or(0),
                    last_vacuum: parts.get(4).and_then(|s| {
                        let v = s.trim();
                        if v.is_empty() {
                            None
                        } else {
                            Some(v.to_string())
                        }
                    }),
                    last_analyze: parts.get(5).and_then(|s| {
                        let v = s.trim();
                        if v.is_empty() {
                            None
                        } else {
                            Some(v.to_string())
                        }
                    }),
                })
            } else {
                None
            }
        })
        .collect();

    let stats = DbStats {
        site_name: site_name.to_string(),
        connections_by_state,
        long_running_queries,
        lock_waits,
        deadlocks_total,
        top_tables,
    };

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&stats).unwrap_or_default()
        );
    } else {
        print_db_stats_human(&stats);
    }

    Ok(stats)
}

fn print_db_stats_human(stats: &DbStats) {
    println!("═══════════════════════════════════════════");
    println!("  DB STATS: {}", stats.site_name);
    println!("═══════════════════════════════════════════\n");

    println!("── Conexiones por estado ──");
    for c in &stats.connections_by_state {
        println!("  {:<20} {}", c.state, c.count);
    }

    if !stats.long_running_queries.is_empty() {
        println!("\n── Queries activas > 5s ──");
        /* [257B-1] Inline string literals to satisfy clippy::print_literal */
        println!("  {:<8} {:<8} {:<10} QUERY", "PID", "DUR(s)", "STATE");
        for q in &stats.long_running_queries {
            let preview = if q.query_preview.len() > 80 {
                &q.query_preview[..80]
            } else {
                &q.query_preview
            };
            println!(
                "  {:<8} {:<8.1} {:<10} {}",
                q.pid, q.duration_secs, q.state, preview
            );
        }
    } else {
        println!("\n✓ No hay queries activas largas");
    }

    if !stats.lock_waits.is_empty() {
        println!("\n── Lock waits ──");
        for l in &stats.lock_waits {
            println!(
                "  Blocked PID {} by PID {} ({})",
                l.blocked_pid, l.blocking_pid, l.lock_type
            );
        }
    }

    println!("\nDeadlocks totales: {}", stats.deadlocks_total);

    if !stats.top_tables.is_empty() {
        println!("\n── Top tablas por tamaño ──");
        /* [257B-1] Inline string literals to satisfy clippy::print_literal */
        println!(
            "  {:<30} {:<12} {:<10} {:<12} LAST_VACUUM",
            "TABLE", "ROWS", "SIZE", "DEAD_TUP"
        );
        for t in &stats.top_tables {
            println!(
                "  {:<30} {:<12} {:<10} {:<12} {}",
                t.table_name,
                t.row_estimate,
                t.total_size,
                t.dead_tuples,
                t.last_vacuum.as_deref().unwrap_or("-")
            );
        }
    }
}
