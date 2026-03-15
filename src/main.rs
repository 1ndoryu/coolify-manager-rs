/*
 * Coolify Manager — punto de entrada principal.
 * Detecta si se invoca en modo CLI o MCP segun argumentos.
 */

mod cli;
mod commands;
mod config;
mod domain;
mod env_loader;
mod error;
mod infra;
mod logging;
mod mcp;
mod services;

use clap::Parser;
use cli::Cli;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let config_path = crate::config::Settings::resolve_config_path(cli.config.as_deref());

    if let Err(error) = env_loader::load_for_config(&config_path) {
        eprintln!("Error cargando .env: {error}");
        std::process::exit(1);
    }

    let is_mcp = cli.mode_is_mcp();
    if let Err(e) = logging::init_with_mode(&cli.log_level, cli.log_dir.as_deref(), is_mcp) {
        eprintln!("Error inicializando logging: {e}");
        std::process::exit(1);
    }

    let result = match is_mcp {
        true => mcp::server::run(&config_path).await,
        false => cli::run(cli).await,
    };

    if let Err(e) = result {
        tracing::error!("{e:#}");
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}
