/*
 * CLI — definicion de comandos con clap.
 * Cada subcomando mapea a un handler en commands/.
 * [257B-1] Agregados comandos de investigación de incidentes.
 */

mod dispatch;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

pub use dispatch::run;

#[derive(Parser)]
#[command(
    name = "coolify-manager",
    version,
    about = "Gestor de despliegues WordPress en Coolify"
)]
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

    /// Inicia en modo MCP (Model Context Protocol) servidor stdio
    #[arg(long, global = true)]
    pub mcp: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

impl Cli {
    /// Detecta si se invoca en modo MCP (flag explícito o sin subcomando).
    pub fn mode_is_mcp(&self) -> bool {
        self.mcp || self.command.is_none()
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

        /// Template de stack (wordpress, kamples, rust, minecraft)
        #[arg(long, default_value = "wordpress")]
        template: String,

        /// Target opcional donde desplegar el sitio
        #[arg(long)]
        target: Option<String>,

        /// [268A-5] Repositorio git para stacks Rust (default: glory-rs).
        /// Proyectos no-glory (p. ej. ong-agape) pasan su propio repo.
        #[arg(long)]
        repo_url: Option<String>,

        /// [268A-5] Binario Rust (Cargo package name) para stacks Rust (default: glory-backend)
        #[arg(long)]
        app_bin: Option<String>,

        /// [268A-5] Directorio del frontend dentro del repo para stacks Rust (default: frontend)
        #[arg(long)]
        frontend_dir: Option<String>,

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

        /// Omitir backup automatico pre-deploy
        #[arg(long)]
        skip_backup: bool,
    },

    /// Deploy zero-downtime para servicios Docker Compose (Rust, etc.)
    DeployService {
        /// Nombre del sitio en settings.json
        #[arg(short, long)]
        name: String,

        /// Omitir build (asume imagen ya construida)
        #[arg(long)]
        skip_build: bool,

        /// Ejecutar seed de datos de prueba post-deploy
        #[arg(long)]
        seed: bool,

        /// No sincronizar compose con Coolify API
        #[arg(long)]
        skip_compose_sync: bool,

        /// Omitir backup pre-deploy
        #[arg(long)]
        skip_backup: bool,
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

    /// Restaura un data directory raw de PostgreSQL (tarball) en un sitio existente
    RestorePgData {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Ruta al tarball del data directory (local o remoto en el VPS)
        #[arg(short, long)]
        file: PathBuf,

        /// Nombre de la base de datos (default: rust_db)
        #[arg(long)]
        database: Option<String>,

        /// Omite el safety snapshot previo
        #[arg(long)]
        skip_safety_snapshot: bool,
    },

    /// Diagnostico completo de un sitio: contenedores, discos, BD, logs y archivos.
    Diagnose {
        /// Nombre del sitio en settings.json
        #[arg(short, long)]
        name: String,

        /// Output en JSON en vez de texto formateado
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// Ejecuta health checks remotos y HTTP del sitio
    Health {
        /// Nombre del sitio (opcional con --all)
        #[arg(short, long, required_unless_present = "all")]
        name: Option<String>,
        /// Verificar todos los sitios
        #[arg(long, default_value_t = false)]
        all: bool,
        /// Enviar alerta por email si un sitio esta caido
        #[arg(long, default_value_t = false)]
        alert: bool,
        /// Reparar fallos recuperables de red en servicios Rust
        #[arg(long, default_value_t = false)]
        repair: bool,
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

    /// [156A-1] Configura DNS completo de un sitio (registros + verificación HTTPS)
    SetupSiteDns {
        /// Nombre del sitio (de settings.json)
        #[arg(short, long)]
        name: String,

        /// IP destino explícita (omite resolución automática)
        #[arg(long)]
        ip: Option<String>,

        /// Solo muestra lo que haría sin ejecutar
        #[arg(long, default_value_t = false)]
        dry_run: bool,

        /// Omite verificación HTTP post-configuración
        #[arg(long, default_value_t = false)]
        skip_verify: bool,
    },

    /// Audita rendimiento y seguridad de la VPS
    Audit {
        /// Target opcional a auditar; si se omite usa la VPS principal
        #[arg(long)]
        target: Option<String>,
    },

    /// Audita el plano de control de Coolify (contenedores core, procesos y logs)
    AuditControlPlane {
        /// Target opcional a auditar; si se omite usa la VPS principal
        #[arg(long)]
        target: Option<String>,

        /// Ventana de logs reciente para inspeccionar el contenedor coolify
        #[arg(long, default_value = "15m")]
        since: String,

        /// Aplica una remediacion conservadora del control-plane antes de reauditar
        #[arg(long, default_value_t = false)]
        repair: bool,
    },

    /// Audita postura de seguridad del host: SSH, firewall, fail2ban y puertos expuestos
    AuditSecurity {
        /// Target opcional a auditar; si se omite usa la VPS principal
        #[arg(long)]
        target: Option<String>,
    },

    /// Audita Redis/THP/overcommit para distinguir latencia propia vs host ruidoso
    AuditRedisLatency {
        /// Target opcional a auditar; si se omite usa la VPS principal
        #[arg(long)]
        target: Option<String>,

        /// Numero de entradas de SLOWLOG a recuperar
        #[arg(long, default_value_t = 10)]
        slowlog_count: u16,
    },

    /// Endurece SSH segun la politica declarada del target y valida rollback seguro
    HardenSsh {
        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,

        /// Solo muestra lo que se aplicaria sin tocar el host
        #[arg(long, default_value_t = false)]
        dry_run: bool,

        /// Aplica el endurecimiento remoto y valida reconexion
        #[arg(long, default_value_t = false)]
        apply: bool,
    },

    /// Aplica firewall host-level y fail2ban segun la politica declarada del target
    EnforceHostSecurity {
        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,

        /// Solo muestra lo que se aplicaria sin tocar el host
        #[arg(long, default_value_t = false)]
        dry_run: bool,

        /// Aplica firewall/fail2ban y valida reconexion
        #[arg(long, default_value_t = false)]
        apply: bool,
    },

    /// Gestiona el plano de control de Coolify en un target sin tocar los sitios alojados
    CoolifyControlPlane {
        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,

        /// Accion: stop, start, status
        #[arg(long, default_value = "status")]
        action: String,

        /// Incluir tambien el proxy de Coolify en la accion
        #[arg(long, default_value_t = false)]
        include_proxy: bool,
    },

    /// Instala Coolify en un target remoto usando SSH
    InstallCoolify {
        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,
    },

    /// Prepara un target remoto como runtime ligero de hosting
    BootstrapTargetLight {
        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,

        /// Solo muestra lo que se haria sin tocar el host
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// Provisiona un hosting normal sobre el runtime ligero
    ProvisionStatic {
        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,

        /// Identificador del sitio o deployment
        #[arg(long)]
        site: String,

        /// Dominio inicial del sitio
        #[arg(long)]
        fqdn: Option<String>,

        /// Usuario SFTP a usar
        #[arg(long)]
        access_user: Option<String>,

        /// Password SFTP a usar
        #[arg(long)]
        access_password: Option<String>,

        /// Emite JSON estable para automatizaciones
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// Lista los sitios detectados en el runtime ligero de un target
    InventoryLight {
        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,

        /// Emite JSON estable para automatizaciones
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// Crea o lista backups remotos de un sitio lightweight
    LightBackup {
        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,

        /// Identificador del sitio o deployment
        #[arg(long)]
        site: String,

        /// Tier remoto del backup: daily, weekly, manual
        #[arg(long, default_value = "manual")]
        tier: String,

        /// Etiqueta opcional para el backup
        #[arg(long)]
        label: Option<String>,

        /// Lista backups existentes en vez de crear uno nuevo
        #[arg(long, default_value_t = false)]
        list: bool,

        /// Emite JSON estable para automatizaciones
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// Restaura un backup remoto sobre un sitio lightweight
    LightRestore {
        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,

        /// Identificador del sitio o deployment
        #[arg(long)]
        site: String,

        /// Identificador del backup remoto
        #[arg(long)]
        backup_id: String,

        /// Password SFTP opcional
        #[arg(long)]
        access_password: Option<String>,

        /// Omite el snapshot de seguridad previo
        #[arg(long, default_value_t = false)]
        skip_safety_snapshot: bool,

        /// Emite JSON estable para automatizaciones
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// Ejecuta una accion sobre un sitio del runtime ligero
    LightSite {
        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,

        /// Identificador del sitio o deployment
        #[arg(long)]
        site: String,

        /// Accion: start, stop, restart, reconfigure, delete
        #[arg(long)]
        action: String,

        /// Dominio final del sitio al reconfigurar
        #[arg(long)]
        fqdn: Option<String>,

        /// Usuario SFTP esperado al reconfigurar
        #[arg(long)]
        access_user: Option<String>,

        /// Password SFTP nueva al reconfigurar
        #[arg(long)]
        access_password: Option<String>,

        /// En delete: elimina tambien el directorio del sitio
        #[arg(long, default_value_t = false)]
        delete_volumes: bool,

        /// Emite JSON estable para automatizaciones
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// Desinstala Coolify de un target remoto
    UninstallCoolify {
        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,

        /// Elimina tambien /data/coolify y los volumenes persistentes
        #[arg(long, default_value_t = false)]
        purge_data: bool,

        /// Solo muestra lo que se haria sin tocar el host
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// Purga workloads Docker remanentes del target
    PurgeDockerHost {
        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,

        /// Limpia tambien volumenes, redes custom, imagenes y builder cache
        #[arg(long, default_value_t = false)]
        all_data: bool,

        /// Solo muestra lo que se haria sin tocar el host
        #[arg(long, default_value_t = false)]
        dry_run: bool,
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

        /// Nueva password admin
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

    /// Ejecuta un comando directamente en el host del VPS via SSH
    HostExec {
        /// Comando bash a ejecutar en el host
        #[arg(long)]
        command: String,

        /// Target del VPS; si se omite usa el principal
        #[arg(long)]
        target: Option<String>,
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

        /// Usar Docker Engine API en vez de SSH
        #[arg(long)]
        docker_socket: Option<String>,

        /// [257B-1] Solo mostrar logs desde este tiempo (ej: 2h, 24h, 2d)
        #[arg(long)]
        since: Option<String>,

        /// [257B-1] Solo mostrar logs hasta este tiempo
        #[arg(long)]
        until: Option<String>,

        /// [257B-1] Filtrar por múltiples patrones (separados por coma)
        #[arg(long)]
        pattern: Option<String>,
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

    /// Redeploy seguro del servicio
    Redeploy {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Omitir backup pre-redeploy
        #[arg(long)]
        skip_backup: bool,
    },

    /// Detecta y corrige mismatch de contraseña entre DATABASE_URL y PostgreSQL
    FixDbAuth {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Solo muestra qué se haría sin aplicar cambios
        #[arg(long)]
        dry_run: bool,
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

        /// Interprete (php, bash, python3)
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
        /// Nombre del sitio
        #[arg(long, short = 'n')]
        name: Option<String>,
        /// Eliminar las tareas programadas
        #[arg(long)]
        remove: bool,
    },

    /// Instala backup-server.sh + crontab en el VPS
    InstallBackups {
        /// Target donde instalar
        #[arg(long)]
        target: Option<String>,

        /// Solo muestra que se haria sin instalar
        #[arg(long, default_value_t = false)]
        dry_run: bool,

        /// Desinstala script y crontab del VPS
        #[arg(long, default_value_t = false)]
        uninstall: bool,
    },

    /// Failover: restaura un sitio en un VPS alternativo usando backup de Drive
    Failover {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Nombre del target destino
        #[arg(long)]
        target: String,

        /// ID de backup especifico
        #[arg(long)]
        backup_id: Option<String>,

        /// Conmuta DNS al target tras health OK
        #[arg(long)]
        switch_dns: bool,

        /// Omite provisionar stack nuevo
        #[arg(long)]
        skip_provision: bool,
    },

    /// Sincroniza variables de entorno entre el .env local y el servicio en Coolify
    SyncEnv {
        /// Nombre del sitio en settings.json
        #[arg(short, long)]
        name: String,

        /// Direccion: diff, push, pull
        #[arg(long, default_value = "diff")]
        direction: String,

        /// Solo muestra diferencias sin aplicar cambios
        #[arg(long)]
        dry_run: bool,

        /// Ruta al archivo .env local
        #[arg(long)]
        env_file: Option<PathBuf>,

        /// Limita diff/push a una o varias claves concretas
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
    },

    /// Prepara Tailscale en el host VPS
    Tailscale {
        /// Target definido en settings.json
        #[arg(long)]
        target: Option<String>,

        /// Auth key explicita de Tailscale
        #[arg(long)]
        auth_key: Option<String>,

        /// Nombre de variable de entorno de donde leer el auth key
        #[arg(long)]
        auth_key_env: Option<String>,

        /// Hostname con el que registrar el VPS en Tailscale
        #[arg(long)]
        hostname: Option<String>,

        /// Tags a anunciar en Tailscale
        #[arg(long)]
        advertise_tags: Option<String>,

        /// Aceptar DNS de Tailscale en el host
        #[arg(long, default_value_t = false)]
        accept_dns: bool,

        /// URL HTTP opcional a probar desde el host
        #[arg(long)]
        probe_url: Option<String>,

        /// Metodo HTTP del probe opcional
        #[arg(long, default_value = "GET")]
        probe_method: String,

        /// Body del probe opcional
        #[arg(long)]
        probe_body: Option<String>,
    },

    /// Aplica optimizaciones host-level repetibles (swap + sysctl)
    OptimizeHost {
        /// Target definido en settings.json
        #[arg(long)]
        target: Option<String>,

        /// Tamano de swap en GB
        #[arg(long, default_value_t = 4)]
        swap_gb: u16,

        /// Valor de vm.swappiness
        #[arg(long, default_value_t = 10)]
        swappiness: u8,

        /// Valor de vm.vfs_cache_pressure
        #[arg(long, default_value_t = 50)]
        vfs_cache_pressure: u16,

        /// Valor de vm.overcommit_memory
        #[arg(long, default_value_t = 1)]
        overcommit_memory: u8,

        /// Desactiva Transparent Huge Pages
        #[arg(long, default_value_t = false)]
        disable_thp: bool,

        /// Persiste live-restore en Docker
        #[arg(long, default_value_t = false)]
        docker_live_restore: bool,

        /// Solo muestra diagnostico sin aplicar
        #[arg(long)]
        dry_run: bool,

        /// Cantidad de muestras para promediar CPU
        #[arg(long, default_value_t = 1)]
        samples: u8,

        /// Segundos entre muestras
        #[arg(long, default_value_t = 5)]
        interval_seconds: u8,
    },

    /// Actualiza paquetes del host remoto
    MaintainHost {
        /// Target definido en settings.json
        #[arg(long)]
        target: Option<String>,

        /// Programa reboot del host
        #[arg(long, default_value_t = false)]
        reboot: bool,

        /// Solo muestra que se haria sin aplicar
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// Evalua la ventana de mantenimiento de un target
    CheckMaintenanceWindow {
        /// Target definido en settings.json
        #[arg(long)]
        target: Option<String>,

        /// Ejecuta el mantenimiento si la decision no queda bloqueada
        #[arg(long, default_value_t = false)]
        apply: bool,

        /// Solo muestra la decision sin aplicar
        #[arg(long, default_value_t = false)]
        dry_run: bool,

        /// Evalua aunque la politica este deshabilitada
        #[arg(long, default_value_t = false)]
        force_evaluate: bool,
    },

    /// Instala o retira el timer remoto de mantenimiento en un target
    ScheduleMaintenance {
        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,

        /// Solo muestra el render de lo que se instalaria
        #[arg(long, default_value_t = false)]
        dry_run: bool,

        /// Elimina timer, service y script remoto
        #[arg(long, default_value_t = false)]
        remove: bool,
    },

    /// Ejecuta SQL arbitrario contra el contenedor PostgreSQL de un sitio
    RunSql {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Query SQL a ejecutar inline
        #[arg(long)]
        query: Option<String>,

        /// Ruta a un archivo .sql local
        #[arg(long)]
        file: Option<PathBuf>,

        /// Envuelve en BEGIN/ROLLBACK
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// Diagnostica la salud de la BD: tablas, migraciones, columnas
    DbCheck {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Tablas esperadas separadas por coma
        #[arg(long)]
        expected_tables: Option<String>,
    },

    /// Aplica migraciones SQL pendientes contra la BD del sitio
    DbMigrate {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Directorio de migraciones
        #[arg(long)]
        migrations_dir: Option<PathBuf>,

        /// Aplica un archivo SQL específico
        #[arg(long)]
        file: Option<PathBuf>,

        /// Envuelve cada migración en BEGIN/ROLLBACK
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// Restaura datos de un cliente vía el endpoint de bootstrap de la API
    RestoreClient {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Email del admin para autenticarse
        #[arg(long, default_value = "andoryyu@gmail.com")]
        admin_email: String,

        /// Password del admin
        #[arg(long)]
        admin_password: String,

        /// Stripe subscription ID a vincular
        #[arg(long)]
        stripe_sub_id: Option<String>,

        /// Solo verifica estado, no ejecuta bootstrap
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    // ──────────────────────────────────────────────────────────────
    // [257B-1] Comandos de investigación de incidentes
    // ──────────────────────────────────────────────────────────────
    /// Investigación completa de incidente: recolecta commit, container, events, logs, health, DB
    IncidentInvestigate {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Output en JSON
        #[arg(long, default_value_t = false)]
        json: bool,

        /// Guarda el reporte en un archivo local (sin secretos)
        #[arg(long)]
        save: Option<PathBuf>,
    },

    /// Busca logs con patrones predefinidos de incidente (FREEZE, panic, OOM, etc.)
    IncidentLogs {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Rango temporal: 24h, 48h, 2d (default: 48h)
        #[arg(long, default_value = "48h")]
        since: String,

        /// Hasta este tiempo
        #[arg(long)]
        until: Option<String>,

        /// Patrones custom adicionales (separados por coma)
        #[arg(long)]
        patterns: Option<String>,

        /// Output en JSON
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// Historial de eventos de ciclo de vida del contenedor (create, start, die, destroy, oom)
    ContainerEvents {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Rango temporal (default: 24h)
        #[arg(long, default_value = "24h")]
        since: String,

        /// Hasta este tiempo
        #[arg(long)]
        until: Option<String>,

        /// Output en JSON
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// Inspecciona estado detallado del contenedor: restart count, OOM, recursos
    ContainerInspect {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Output en JSON
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// Métricas de recursos del contenedor: CPU, memoria, red, disco
    ContainerStats {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Output en JSON
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// Métricas rápidas de PostgreSQL: conexiones, queries lentas, locks, tablas
    DbStats {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Umbral de segundos para queries largas (default: 5)
        #[arg(long, default_value_t = 5)]
        threshold: u32,

        /// Output en JSON
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// Compara la BD de un sitio contra un dump o contra otro sitio (E12)
    DbCompare {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Ruta al dump (local o VPS). Si se omite y no hay --against, usa el último dump VPS
        #[arg(long)]
        dump: Option<String>,

        /// Nombre de otro sitio configurado para comparar en vivo
        #[arg(long)]
        against: Option<String>,

        /// Limitar a tablas concretas (separadas por coma)
        #[arg(long)]
        tables: Option<String>,

        /// Columnas volátiles a ignorar (separadas por coma)
        #[arg(long)]
        ignore_columns: Option<String>,

        /// Máx filas de muestra por tabla (default: 20)
        #[arg(long, default_value_t = 20)]
        limit_diff: usize,

        /// Output en JSON
        #[arg(long, default_value_t = false)]
        json: bool,

        /// Modo ligero: solo conteos + hashes sin contenedor temporal
        #[arg(long, default_value_t = false)]
        no_tmp_container: bool,

        /// Máx filas a extraer por tabla (seguridad, default: todas)
        #[arg(long)]
        extract_limit: Option<u64>,
    },

    /// Cambia rápidamente una variable de entorno en Coolify (para mitigación de incidentes)
    EnvToggle {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Nombre de la variable
        #[arg(long)]
        key: String,

        /// Nuevo valor
        #[arg(long)]
        value: String,

        /// Reiniciar el servicio después del cambio
        #[arg(long, default_value_t = false)]
        restart: bool,

        /// Solo muestra qué haría sin aplicar
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// Muestra la ruta de settings.json resuelta por el binario actual
    GetConfigPath,

    /// Inicia API HTTP local para usar la GUI web sin Tauri
    GuiApi {
        /// Direccion local de escucha
        #[arg(long, default_value = "127.0.0.1:8787")]
        bind: std::net::SocketAddr,
    },
}
