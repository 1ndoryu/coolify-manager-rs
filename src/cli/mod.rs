/*
 * CLI — definicion de comandos con clap.
 * Cada subcomando mapea a un handler en commands/.
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

    /// Prepara un target remoto como runtime ligero de hosting (Docker + Caddy + MariaDB + Redis)
    BootstrapTargetLight {
        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,

        /// Solo muestra lo que se haria sin tocar el host
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// Desinstala Coolify de un target remoto y opcionalmente purga datos persistentes
    UninstallCoolify {
        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,

        /// Elimina tambien /data/coolify y los volumenes persistentes de Coolify
        #[arg(long, default_value_t = false)]
        purge_data: bool,

        /// Solo muestra lo que se haria sin tocar el host
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// Purga workloads Docker remanentes del target y opcionalmente limpia imagenes/cache
    PurgeDockerHost {
        /// Nombre del target definido en settings.json
        #[arg(long)]
        target: String,

        /// Limpia tambien volumenes, redes custom, imagenes no usadas y builder cache
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

    /// Redeploy seguro del servicio; en stacks Rust delega al mismo flujo protegido que deploy
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

    /// Failover: restaura un sitio en un VPS alternativo usando backup de Drive (no requiere VPS origen)
    Failover {
        /// Nombre del sitio
        #[arg(short, long)]
        name: String,

        /// Nombre del target destino definido en settings.json
        #[arg(long)]
        target: String,

        /// ID de backup especifico; si se omite usa el mas reciente en Drive
        #[arg(long)]
        backup_id: Option<String>,

        /// Conmuta DNS al target tras health OK
        #[arg(long)]
        switch_dns: bool,

        /// Omite provisionar stack nuevo (usa stackUuid existente del sitio)
        #[arg(long)]
        skip_provision: bool,
    },

    /// Sincroniza variables de entorno entre el .env local y el servicio en Coolify
    SyncEnv {
        /// Nombre del sitio en settings.json
        #[arg(short, long)]
        name: String,

        /// Direccion: diff (solo mostrar), push (local->Coolify), pull (Coolify->local)
        #[arg(long, default_value = "diff")]
        direction: String,

        /// Solo muestra diferencias sin aplicar cambios
        #[arg(long)]
        dry_run: bool,

        /// Ruta al archivo .env local (por defecto auto-detecta en raiz del proyecto)
        #[arg(long)]
        env_file: Option<PathBuf>,

        /// Limita diff/push a una o varias claves concretas. Acepta repetido o separado por comas.
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
    },

    /// Prepara Tailscale en el host VPS y opcionalmente prueba reachability a un endpoint privado
    Tailscale {
        /// Target definido en settings.json; si se omite usa la VPS principal
        #[arg(long)]
        target: Option<String>,

        /// Auth key explicita de Tailscale para alta no interactiva
        #[arg(long)]
        auth_key: Option<String>,

        /// Nombre de variable de entorno de donde leer el auth key
        #[arg(long)]
        auth_key_env: Option<String>,

        /// Hostname con el que registrar el VPS en Tailscale
        #[arg(long)]
        hostname: Option<String>,

        /// Tags a anunciar en Tailscale (ej: tag:vps,tag:prod)
        #[arg(long)]
        advertise_tags: Option<String>,

        /// Aceptar DNS de Tailscale en el host
        #[arg(long, default_value_t = false)]
        accept_dns: bool,

        /// URL HTTP opcional a probar desde el host una vez autenticado
        #[arg(long)]
        probe_url: Option<String>,

        /// Metodo HTTP del probe opcional
        #[arg(long, default_value = "GET")]
        probe_method: String,

        /// Body del probe opcional
        #[arg(long)]
        probe_body: Option<String>,
    },

    /// Aplica optimizaciones host-level repetibles (swap + sysctl) y reporta procesos calientes
    OptimizeHost {
        /// Target definido en settings.json; si se omite usa la VPS principal
        #[arg(long)]
        target: Option<String>,

        /// Tamano de swap en GB a asegurar cuando el host no tiene swap activa
        #[arg(long, default_value_t = 4)]
        swap_gb: u16,

        /// Valor de vm.swappiness a dejar persistente
        #[arg(long, default_value_t = 10)]
        swappiness: u8,

        /// Valor de vm.vfs_cache_pressure a dejar persistente
        #[arg(long, default_value_t = 50)]
        vfs_cache_pressure: u16,

        /// Valor de vm.overcommit_memory a dejar persistente
        #[arg(long, default_value_t = 1)]
        overcommit_memory: u8,

        /// Desactiva Transparent Huge Pages en runtime y de forma persistente
        #[arg(long, default_value_t = false)]
        disable_thp: bool,

        /// Persiste live-restore en Docker para futuros reloads/restarts del daemon
        #[arg(long, default_value_t = false)]
        docker_live_restore: bool,

        /// Solo muestra diagnostico y cambios planeados sin aplicarlos
        #[arg(long)]
        dry_run: bool,

        /// Cantidad de muestras para promediar CPU de procesos y contenedores
        #[arg(long, default_value_t = 1)]
        samples: u8,

        /// Segundos entre muestras cuando samples > 1
        #[arg(long, default_value_t = 5)]
        interval_seconds: u8,
    },

    /// Actualiza paquetes del host remoto y opcionalmente programa un reinicio
    MaintainHost {
        /// Target definido en settings.json; si se omite usa la VPS principal
        #[arg(long)]
        target: Option<String>,

        /// Programa reboot del host al finalizar la actualizacion
        #[arg(long, default_value_t = false)]
        reboot: bool,

        /// Solo muestra que se haria sin aplicar cambios
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// Evalua la ventana de mantenimiento de un target y opcionalmente ejecuta el mantenimiento
    CheckMaintenanceWindow {
        /// Target definido en settings.json; si se omite usa la VPS principal
        #[arg(long)]
        target: Option<String>,

        /// Ejecuta el mantenimiento si la decision no queda bloqueada
        #[arg(long, default_value_t = false)]
        apply: bool,

        /// Solo muestra la decision sin aplicar cambios
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

        /// Elimina timer, service y script remoto en vez de instalarlos
        #[arg(long, default_value_t = false)]
        remove: bool,
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
