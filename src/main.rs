/*
 * Coolify Manager — punto de entrada del binario CLI/MCP.
 * Usa la libreria coolify_manager para toda la logica de negocio.
 * Solo el modulo cli es exclusivo del binario.
 */

mod cli;

use clap::Parser;
use cli::Cli;
use coolify_manager::{config, env_loader, logging, mcp};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let config_path = config::Settings::resolve_config_path(cli.config.as_deref());

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
