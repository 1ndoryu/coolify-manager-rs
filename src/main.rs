/*
 * Coolify Manager — punto de entrada del binario CLI/MCP.
 * Usa la libreria coolify_manager para toda la logica de negocio.
 * Solo el modulo cli es exclusivo del binario.
 */

mod cli;

use clap::Parser;
use cli::Cli;
use coolify_manager::{config, env_loader, logging, mcp};

const MAIN_THREAD_STACK_SIZE: usize = 16 * 1024 * 1024;

fn main() {
    /* [105A-7] Clap + el enum grande del CLI pueden desbordar la pila por defecto en Windows. */
    let thread = std::thread::Builder::new()
        .name("coolify-manager-main".to_string())
        .stack_size(MAIN_THREAD_STACK_SIZE)
        .spawn(run_main);

    match thread {
        Ok(handle) => match handle.join() {
            Ok(exit_code) => std::process::exit(exit_code),
            Err(_) => {
                eprintln!("Error: hilo principal de coolify-manager termino con panic");
                std::process::exit(1);
            }
        },
        Err(error) => {
            eprintln!("Error iniciando coolify-manager: {error}");
            std::process::exit(1);
        }
    }
}

fn run_main() -> i32 {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Error inicializando runtime async: {error}");
            return 1;
        }
    };

    runtime.block_on(async_main())
}

async fn async_main() -> i32 {
    let cli = Cli::parse();
    let config_path = config::Settings::resolve_config_path(cli.config.as_deref());

    if let Err(error) = env_loader::load_for_config(&config_path) {
        eprintln!("Error cargando .env: {error}");
        return 1;
    }

    let is_mcp = cli.mode_is_mcp();
    if let Err(e) = logging::init_with_mode(&cli.log_level, cli.log_dir.as_deref(), is_mcp) {
        eprintln!("Error inicializando logging: {e}");
        return 1;
    }

    let result = match is_mcp {
        true => mcp::server::run(&config_path).await,
        false => cli::run(cli).await,
    };

    if let Err(e) = result {
        tracing::error!("{e:#}");
        eprintln!("Error: {e:#}");
        return 1;
    }

    0
}
