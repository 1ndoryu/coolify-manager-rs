/*
 * CLI — definicion de comandos con clap.
 * Cada subcomando mapea a un handler en commands/.
 */

use crate::commands;
use crate::error::CoolifyError;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "coolify-manager", version, about = "Gestor de despliegues WordPress en Coolify")]
pub struct Cli {
    /// Nivel de logging (trace, debug, info, warn, error)
    #[arg(long, default_value = "info", global = true)]
    pub log_level: String,

    /// Directorio para archivos de log
    #[arg(long, global = true)]
    pub log_dir: Option<String>,

    /// Ruta al archivo de configuracion (settings.json)
    #[arg(long, short = 'c', global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

impl Cli {
    /// Detecta si se invoca en modo MCP (sin subcomando, con stdin pipe).
    pub fn mode_is_mcp(&self) -> bool {
        self.command.is_none()
    }
}

#[derive(Subcommand)]
pub enum Command {
    /// Crea un nuevo sitio WordPress con tema Glory en Coolify
    New {
        /// Nombre unico del sitio (slug)
        #[arg(short, long)]
        name: String,

        /// Dominio completo con protocolo (https://...)
        #[arg(short, long)]
        domain: String,

        /// Rama del tema Glory
        #[arg(long, default_value = "main")]
        glory_branch: String,

        /// Rama de la libreria Glory
        #[arg(long, default_value = "main")]
        library_branch: String,

        /// Template de stack (wordpress, kamples, minecraft)
        #[arg(long, default_value = "wordpress")]
        template: String,

        /// Target opcional donde desplegar el sitio
        #[arg(long)]
        target: Option<String>,

        /// Omitir instalacion del tema
        #[arg(long)]
        skip_theme: bool,

        /// Omitir configuracion de cache headers
        #[arg(long)]
        skip_cache: bool,
    },

    /// Despliega o actualiza el tema Glory en un sitio existente
    Deploy {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Rama del tema Glory
        #[arg(long)]
        glory_branch: Option<String>,

        /// Rama de la libreria Glory
        #[arg(long)]
        library_branch: Option<String>,

        /// Actualiza en vez de reinstalar
        #[arg(long)]
        update: bool,

        /// Omitir compilacion de React
        #[arg(long)]
        skip_react: bool,

        /// Fuerza git reset --hard antes de pull
        #[arg(long)]
        force: bool,
    },

    /// Lista todos los sitios configurados
    List {
        /// Muestra informacion adicional
        #[arg(long)]
        detailed: bool,
    },

    /// Reinicia los servicios de un sitio
    Restart {
        /// Nombre del sitio
        #[arg(short, long)]
        name: Option<String>,

        /// Reinicia todos los sitios
        #[arg(long)]
        all: bool,

        /// Solo reinicia contenedor de BD
        #[arg(long)]
        only_db: bool,

        /// Solo reinicia contenedor WordPress
        #[arg(long)]
        only_wordpress: bool,
    },

    /// Importa un archivo SQL en la base de datos del sitio
    Import {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Ruta local al archivo .sql
        #[arg(short, long)]
        file: PathBuf,

        /// Corregir URLs al dominio configurado tras importar
        #[arg(long)]
        fix_urls: bool,
    },

    /// Exporta la base de datos del sitio a un archivo SQL
    Export {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Ruta local de salida
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Crea o lista copias de seguridad externas del sitio
    Backup {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Tier de backup: daily, weekly, manual
        #[arg(long, default_value = "manual")]
        tier: String,

        /// Etiqueta opcional para el backup
        #[arg(long)]
        label: Option<String>,

        /// Lista backups existentes en vez de crear uno nuevo
        #[arg(long)]
        list: bool,
    },

    /// Restaura un backup especifico en un sitio
    Restore {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Identificador del backup
        #[arg(long)]
        backup_id: String,

        /// Omite snapshot de seguridad previo
        #[arg(long)]
        skip_safety_snapshot: bool,
    },

    /// Ejecuta health checks remotos y HTTP del sitio
    Health {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,
    },

    /// Migra un sitio completo a otro target configurado
    Migrate {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,

        /// Solo genera y valida el plan sin ejecutar
        #[arg(long)]
        dry_run: bool,

        /// Conmuta DNS al target tras health OK
        #[arg(long)]
        switch_dns: bool,
    },

    /// Conmuta los registros DNS del sitio hacia una IP o target
    SwitchDns {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Target definido en settings.json para tomar su IP
        #[arg(long)]
        target: Option<String>,

        /// IP explícita destino
        #[arg(long)]
        ip: Option<String>,

        /// Solo muestra acciones sin aplicarlas
        #[arg(long)]
        dry_run: bool,
    },

    /// Audita rendimiento y seguridad de la VPS
    Audit {
        /// Target opcional a auditar; si se omite usa la VPS principal
        #[arg(long)]
        target: Option<String>,
    },

    /// Instala Coolify en un target remoto usando SSH
    InstallCoolify {
        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,
    },

    /// Audita seguridad WordPress o rota password admin
    WpSecurity {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Ejecuta auditoría de seguridad
        #[arg(long)]
        audit: bool,

        /// Usuario admin cuya password se va a rotar
        #[arg(long)]
        user: Option<String>,

        /// Nueva password admin; si se omite se genera una aleatoria
        #[arg(long)]
        password: Option<String>,
    },

    /// Ejecuta un comando en el contenedor del sitio
    Exec {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Comando bash a ejecutar
        #[arg(long)]
        command: Option<String>,

        /// Codigo PHP a ejecutar
        #[arg(long)]
        php: Option<String>,

        /// Contenedor objetivo (wordpress, mariadb, postgres)
        #[arg(long, default_value = "wordpress")]
        target: String,
    },

    /// Ver logs del contenedor o debug.log de WordPress
    Logs {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Numero de lineas a mostrar
        #[arg(short, long, default_value = "50")]
        lines: u32,

        /// Contenedor objetivo
        #[arg(long, default_value = "wordpress")]
        target: String,

        /// Ver debug.log en vez de container logs
        #[arg(long)]
        wp_debug: bool,

        /// Filtrar por patron
        #[arg(long)]
        filter: Option<String>,
    },

    /// Activa o desactiva WP_DEBUG
    Debug {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Habilita WP_DEBUG
        #[arg(long)]
        enable: bool,

        /// Deshabilita WP_DEBUG
        #[arg(long)]
        disable: bool,

        /// Muestra estado actual
        #[arg(long)]
        status: bool,
    },

    /// Gestiona cache headers HTTP del sitio
    Cache {
        /// Nombre del sitio
        #[arg(short, long)]
        name: Option<String>,

        /// Accion: status, enable, disable
        #[arg(short, long)]
        action: String,

        /// Aplica a todos los sitios
        #[arg(long)]
        all: bool,
    },

    /// Muestra estado de Git en el tema Glory remoto
    GitStatus {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,
    },

    /// Cambia el dominio de un sitio WordPress
    SetDomain {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Nuevo dominio con protocolo
        #[arg(short, long)]
        domain: String,
    },

    /// Fuerza un redeploy del servicio via Coolify API
    Redeploy {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,
    },

    /// Agrega servicio WebSocket (Bun) a un stack Kamples existente
    DeployWebsocket {
        /// Nombre del sitio Kamples
        #[arg(short, long)]
        name: String,
    },

    /// Sube un script local al contenedor y lo ejecuta
    RunScript {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Ruta al script local
        #[arg(short, long)]
        file: PathBuf,

        /// Interprete (php, bash, python3). Auto-detecta por extension si se omite
        #[arg(short, long)]
        interpreter: Option<String>,

        /// Contenedor objetivo (wordpress, mariadb)
        #[arg(long, default_value = "wordpress")]
        target: String,

        /// Argumentos adicionales para el script
        #[arg(long)]
        args: Option<String>,
    },

    /// Configura SMTP relay en el sitio WordPress
    Smtp {
        /// Nombre del sitio
        #[arg(short, long)]
        name: Option<String>,

        /// Configura SMTP en todos los sitios
        #[arg(long)]
        all: bool,

        /// Envia correo de prueba
        #[arg(long)]
        test: bool,

        /// Email destino para prueba
        #[arg(long)]
        test_email: Option<String>,

        /// Muestra estado actual
        #[arg(long)]
        status: bool,
    },

    /// Gestiona servidores Minecraft
    Minecraft {
        /// Accion: new, logs, console, restart, status, remove
        #[arg(short, long)]
        action: String,

        /// Nombre del servidor
        #[arg(short = 's', long)]
        server_name: String,

        /// RAM asignada
        #[arg(long, default_value = "2G")]
        memory: String,

        /// Max jugadores
        #[arg(long, default_value = "20")]
        max_players: u32,

        /// Dificultad
        #[arg(long, default_value = "normal")]
        difficulty: String,

        /// Version de Minecraft
        #[arg(long, default_value = "LATEST")]
        version: String,

        /// Puerto externo
        #[arg(long, default_value = "25565")]
        port: u16,

        /// Comando MC (solo con action=console)
        #[arg(long)]
        console_command: Option<String>,

        /// Lineas de log
        #[arg(long, default_value = "100")]
        lines: u32,
    },

    /// Autoriza Google Drive con tu cuenta personal (OAuth)
    AuthDrive,

    /// Registra/elimina tareas de backup automaticas en Windows Task Scheduler
    ScheduleBackup {
        /// Nombre del sitio (si se omite, procesa todos los habilitados)
        #[arg(long, short = 'n')]
        name: Option<String>,
        /// Eliminar las tareas programadas en vez de crearlas
        #[arg(long)]
        remove: bool,
    },
}

/// Punto de entrada del CLI — enruta al handler correspondiente.
pub async fn run(cli: Cli) -> std::result::Result<(), CoolifyError> {
    let config_path = crate::config::Settings::resolve_config_path(cli.config.as_deref());

    match cli.command {
        Some(Command::New {
            name,
            domain,
            glory_branch,
            library_branch,
            template,
            target,
            skip_theme,
            skip_cache,
        }) => {
            commands::new_site::execute(&config_path, &name, &domain, &glory_branch, &library_branch, &template, target.as_deref(), skip_theme, skip_cache).await
        }
        Some(Command::Deploy {
            name,
            glory_branch,
            library_branch,
            update,
            skip_react,
            force,
        }) => {
            commands::deploy_theme::execute(&config_path, &name, glory_branch.as_deref(), library_branch.as_deref(), update, skip_react, force).await
        }
        Some(Command::List { detailed }) => {
            commands::list_sites::execute(&config_path, detailed).await
        }
        Some(Command::Restart {
            name,
            all,
            only_db,
            only_wordpress,
        }) => {
            commands::restart_site::execute(&config_path, name.as_deref(), all, only_db, only_wordpress).await
        }
        Some(Command::Import {
            name,
            file,
            fix_urls,
        }) => {
            commands::import_database::execute(&config_path, &name, &file, fix_urls).await
        }
        Some(Command::Export { name, output }) => {
            commands::export_database::execute(&config_path, &name, output.as_deref()).await
        }
        Some(Command::Backup {
            name,
            tier,
            label,
            list,
        }) => {
            commands::backup_site::execute(&config_path, &name, &tier, label.as_deref(), list).await
        }
        Some(Command::Restore {
            name,
            backup_id,
            skip_safety_snapshot,
        }) => {
            commands::restore_backup::execute(&config_path, &name, &backup_id, skip_safety_snapshot).await
        }
        Some(Command::Health { name }) => {
            commands::health_check::execute(&config_path, &name).await
        }
        Some(Command::Migrate {
            name,
            target,
            dry_run,
            switch_dns,
        }) => {
            commands::migrate_site::execute(&config_path, &name, &target, dry_run, switch_dns).await
        }
        Some(Command::SwitchDns {
            name,
            target,
            ip,
            dry_run,
        }) => {
            commands::switch_dns::execute(&config_path, &name, target.as_deref(), ip.as_deref(), dry_run).await
        }
        Some(Command::Audit { target }) => {
            commands::audit_vps::execute(&config_path, target.as_deref()).await
        }
        Some(Command::InstallCoolify { target }) => {
            commands::install_coolify::execute(&config_path, &target).await
        }
        Some(Command::WpSecurity {
            name,
            audit,
            user,
            password,
        }) => {
            commands::wordpress_security::execute(&config_path, &name, audit, user.as_deref(), password.as_deref()).await
        }
        Some(Command::Exec {
            name,
            command,
            php,
            target,
        }) => {
            commands::exec_command::execute(&config_path, &name, command.as_deref(), php.as_deref(), &target).await
        }
        Some(Command::Logs {
            name,
            lines,
            target,
            wp_debug,
            filter,
        }) => {
            commands::view_logs::execute(&config_path, &name, lines, &target, wp_debug, filter.as_deref()).await
        }
        Some(Command::Debug {
            name,
            enable,
            disable,
            status,
        }) => {
            commands::debug_site::execute(&config_path, &name, enable, disable, status).await
        }
        Some(Command::Cache { name, action, all }) => {
            commands::cache_site::execute(&config_path, name.as_deref(), &action, all).await
        }
        Some(Command::GitStatus { name }) => {
            commands::git_status::execute(&config_path, &name).await
        }
        Some(Command::SetDomain { name, domain }) => {
            commands::set_domain::execute(&config_path, &name, &domain).await
        }
        Some(Command::Redeploy { name }) => {
            commands::redeploy::execute(&config_path, &name).await
        }
        Some(Command::DeployWebsocket { name }) => {
            commands::deploy_websocket::execute(&config_path, &name).await
        }
        Some(Command::RunScript {
            name,
            file,
            interpreter,
            target,
            args,
        }) => {
            commands::run_script::execute(&config_path, &name, &file, interpreter.as_deref(), &target, args.as_deref()).await
        }
        Some(Command::Smtp {
            name,
            all,
            test,
            test_email,
            status,
        }) => {
            commands::setup_smtp::execute(&config_path, name.as_deref(), all, test, test_email.as_deref(), status).await
        }
        Some(Command::Minecraft {
            action,
            server_name,
            memory,
            max_players,
            difficulty,
            version,
            port,
            console_command,
            lines,
        }) => {
            commands::minecraft::execute(
                &config_path, &action, &server_name, &memory, max_players,
                &difficulty, &version, port, console_command.as_deref(), lines,
            ).await
        }
        Some(Command::AuthDrive) => {
            commands::auth_drive::execute(&config_path).await
        }
        Some(Command::ScheduleBackup { name, remove }) => {
            commands::schedule_backup::execute(&config_path, name.as_deref(), remove).await
        }
        None => {
            /* Modo MCP — se maneja en main.rs */
            Ok(())
        }
    }
}
