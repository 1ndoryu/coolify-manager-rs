/*
 * Sistema de logging con tracing.
 * CLI: consola (stdout) + archivo opcional.
 * MCP: solo archivo (o stderr si no hay log_dir). Stdout reservado para JSON-RPC.
 */

use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use std::path::Path;

pub fn init(level: &str, log_dir: Option<&str>) -> std::result::Result<(), Box<dyn std::error::Error>> {
    init_with_mode(level, log_dir, false)
}

pub fn init_with_mode(level: &str, log_dir: Option<&str>, mcp_mode: bool) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let make_filter = || EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));

    match (log_dir, mcp_mode) {
        (Some(dir), true) => {
            /* MCP + archivo: solo file layer, stdout queda limpio para JSON-RPC */
            let log_path = Path::new(dir);
            if !log_path.exists() { std::fs::create_dir_all(log_path)?; }
            let appender = tracing_appender::rolling::daily(log_path, "coolify-manager.log");
            tracing_subscriber::registry()
                .with(make_filter())
                .with(fmt::layer().with_writer(appender).with_ansi(false).json())
                .init();
        }
        (Some(dir), false) => {
            /* CLI + archivo: consola stdout + file */
            let log_path = Path::new(dir);
            if !log_path.exists() { std::fs::create_dir_all(log_path)?; }
            let appender = tracing_appender::rolling::daily(log_path, "coolify-manager.log");
            tracing_subscriber::registry()
                .with(make_filter())
                .with(fmt::layer().with_target(false).with_thread_ids(false).compact())
                .with(fmt::layer().with_writer(appender).with_ansi(false).json())
                .init();
        }
        (None, true) => {
            /* MCP sin archivo: tracing a stderr para no contaminar stdout */
            tracing_subscriber::registry()
                .with(make_filter())
                .with(fmt::layer().with_writer(std::io::stderr).with_target(false).with_thread_ids(false).compact())
                .init();
        }
        (None, false) => {
            /* CLI sin archivo: consola stdout */
            tracing_subscriber::registry()
                .with(make_filter())
                .with(fmt::layer().with_target(false).with_thread_ids(false).compact())
                .init();
        }
    }

    Ok(())
}
