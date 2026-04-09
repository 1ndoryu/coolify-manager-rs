# coolify-manager-rs

Herramienta de gestion para sitios WordPress en Coolify — reescritura completa en Rust del coolify-manager PowerShell original.

Incluye: backup automatizado con SSH a VPS remoto (o Google Drive legacy), restore validado, health checks con alertas por email, migracion entre targets, failover sin VPS origen, auditoria de VPS, deploy protegido con rollback y pre-deploy safety check, 26+ herramientas MCP para VS Code, y GUI desktop con Tauri v2 + React.

## Arquitectura

```
           ┌── CLI (clap)
lib.rs ──> ├── MCP (JSON-RPC 2.0)  ──> Commands ──> Services ──> Infrastructure
           └── GUI (Tauri v2)                                      │
                                                     SSH, API, Docker,
                                                     templates, secrets
```

Dual target: **library** (`lib.rs`) + **binary** (`main.rs`). La GUI y el MCP consumen la misma API.

- **API** (`src/api/`): Funciones estructuradas con tipos serializables (SiteSummary, HealthResponse, etc.).
- **CLI** (`src/cli/`): Parser clap con subcomandos operativos y de recuperacion.
- **MCP** (`src/mcp/`): Servidor JSON-RPC 2.0 sobre stdio (26 tools). Tracing a stderr/file, stdout limpio.
- **Commands** (`src/commands/`): 29 handlers individuales (incluye failover, deploy-websocket).
- **Services** (`src/services/`): Logica de negocio (temas, DB, cache, rollback, backups, health, migracion, auditoria).
- **Infrastructure** (`src/infra/`): SSH nativo (russh), Coolify API (reqwest), Docker, templates, secrets.
- **GUI** (`gui/`): Tauri v2 + React 19 desktop app (Cargo workspace member).

## Requisitos

- **Rust 1.70+** (probado con 1.94)
- **Cargo** (incluido con rustup)

## Build

```bash
cargo build --release
```

El binario se genera en `target/release/coolify-manager.exe` (~10 MB).

## Variables de entorno

El binario carga `.env` y `.env.local` automáticamente desde la raíz del proyecto antes de leer `config/settings.json`.

`config/settings.json` puede usar `${VAR}` y esas variables se expanden contra el entorno ya cargado.

Usa `.env.example` como plantilla. El archivo real `.env` queda ignorado por git.

If `backupStorage.remote.type = sshremote`, cada backup validado se empaqueta y se transfiere al VPS remoto via SSH. Tambien se soporta `googledrive` como backend legacy.

## Tests

```bash
cargo test
```

62 tests unitarios cubriendo: configuracion, validacion, templates, rollback, domain, errores, secrets, carga de entorno, SSH encoding, Google Drive, SSH backup, utilidades del sistema de backup y API.

## Cambios recientes (abril 2026)

### Backup SSH a VPS remoto (reemplaza Google Drive)
- Nuevo backend `sshremote` para almacenamiento de backups en un VPS secundario via SSH
- Transferencia eficiente de archivos grandes via channel piping (sin limite de ARG_MAX)
- Verificacion de integridad post-upload (comparacion de tamano)
- Estructura remota: `{baseDir}/{sitio}/{tier}/{backup_id}.tar.gz`
- Google Drive se mantiene como opcion legacy pero ya no es el default
- Configuracion en settings.json: `backupStorage.remote.type = "sshremote"`

### Alertas por email
- Nuevo servicio `alert_manager` con envio SMTP via lettre (Brevo/Sendinblue)
- `health --all`: verifica todos los sitios de un solo comando
- `health --alert`: envia email automatico cuando un sitio esta caido
- Resumen consolidado si multiples sitios con problemas
- Usa la configuracion SMTP global de settings.json

### Glory sync en instalacion
- `glory_sync.php` se ejecuta automaticamente 3 veces al instalar tema
- Sincroniza opciones, paginas y contenido por defecto de Glory
- Busca el script en `scripts/` y `Glory/scripts/` del tema

### Deploy protegido (F1-F12 fixes)
- Pre-deploy safety check: verifica todos los sitios en Coolify API antes de deployar
- Backup automatico pre-deploy (flag `--skip-backup` para omitir)
- Health check de TODOS los sitios post-deploy (no solo el deployado)
- Rollback completo: git reset + composer + npm install + build
- Persistencia de tema: dockerfile_inline con deps + entrypoint self-healing
- Labels Traefik explicitos en templates (no dependen de sslip.io)
- Verificacion de contenido HTML en health check (detecta tema por defecto)
- Mejor diagnostico de errores de build (stdout+stderr + hint para exit 127)

## Uso CLI

```bash
# Ver ayuda
coolify-manager --help

# Crear un sitio nuevo
coolify-manager new --name mi-sitio --domain https://mi-sitio.com

# Desplegar tema Glory
coolify-manager deploy --name mi-sitio

# Listar sitios
coolify-manager list

# Ver logs
coolify-manager logs --name mi-sitio --lines 50

# Importar base de datos
coolify-manager import --name mi-sitio --file backup.sql

# Exportar base de datos
coolify-manager export --name mi-sitio --output backup.sql

# Crear backup externo manual
coolify-manager backup --name mi-sitio --tier manual --label antes-de-update

# Si hay Google Drive configurado, la subida remota ocurre automaticamente

# Listar backups del sitio
coolify-manager backup --name mi-sitio --list

# Restaurar un backup concreto
coolify-manager restore --name mi-sitio --backup-id 20260314_120000-antes_de_update

# Ejecutar health checks
coolify-manager health --name mi-sitio

# Verificar TODOS los sitios y enviar alerta por email si alguno esta caido
coolify-manager health --all --alert

# Migrar un sitio a otro target configurado
# El dry-run ahora hace preflight real: valida origen/destino sin copiar datos
coolify-manager migrate --name mi-sitio --target produccion-b --dry-run
coolify-manager migrate --name mi-sitio --target standby-vps2 --switch-dns

# Conmutar DNS manualmente a una IP o target
coolify-manager switch-dns --name mi-sitio --target standby-vps2 --dry-run
coolify-manager switch-dns --name mi-sitio --ip 173.249.50.44

# Auditar VPS principal o target
coolify-manager audit
coolify-manager audit --target produccion-b

# Instalar Coolify en un target standby nuevo
coolify-manager install-coolify --target standby-vps2

# Auditar seguridad WordPress y rotar password admin
coolify-manager wp-security --name mi-sitio --audit
coolify-manager wp-security --name mi-sitio --user admin

# Debug mode
coolify-manager debug --name mi-sitio --enable
coolify-manager debug --name mi-sitio --disable

# Reiniciar sitio
coolify-manager restart --name mi-sitio

# Ejecutar comando remoto
coolify-manager exec --name mi-sitio --command "wp option get siteurl"

# Cambiar dominio
coolify-manager set-domain --name mi-sitio --domain https://nuevo.com

# Cache headers
coolify-manager cache --name mi-sitio --enable
coolify-manager cache --name mi-sitio --disable

# Git status remoto
coolify-manager git-status --name mi-sitio

# Redeploy via Coolify API
coolify-manager redeploy --name mi-sitio

# Configurar SMTP
coolify-manager smtp --name mi-sitio --host smtp.gmail.com --port 587

# Failover: restaurar sitio en VPS alternativo desde backup Drive
coolify-manager failover --name mi-sitio --target standby-vps2

# Deploy WebSocket container
coolify-manager deploy-websocket --name mi-sitio

# Schedule backup automatizado (Windows Task Scheduler)
coolify-manager schedule-backup --name mi-sitio --tier daily --cron "0 3 * * *"

# Minecraft
coolify-manager minecraft --action new --server-name survival --memory 4G
coolify-manager minecraft --action logs --server-name survival
```

## Uso como MCP Server (VS Code)

```bash
coolify-manager --mcp
```

Se comunica por stdin/stdout con JSON-RPC 2.0 (protocolo MCP). Configurar en `.vscode/mcp.json`:

```json
{
    "servers": {
        "coolify-manager": {
            "type": "stdio",
            "command": "${workspaceFolder}/.agent/coolify-manager-rs/target/release/coolify-manager.exe",
            "args": ["--mcp"]
        }
    }
}
```

### Herramientas MCP disponibles

| Herramienta       | Descripcion                    |
| ----------------- | ------------------------------ |
| `new_site`        | Crear sitio WordPress          |
| `deploy_theme`    | Desplegar tema Glory           |
| `list_sites`      | Listar sitios configurados     |
| `restart_site`    | Reiniciar servicios            |
| `import_database` | Importar SQL                   |
| `export_database` | Exportar SQL                   |
| `coolify_backup`  | Crear o listar backups externos|
| `coolify_restore_backup` | Restaurar backup validado |
| `coolify_health`  | Ejecutar health checks         |
| `coolify_migrate` | Migrar sitio a otro target     |
| `coolify_audit_vps` | Auditar VPS o target         |
| `coolify_wp_security` | Auditar WordPress y rotar admin |
| `exec_command`    | Ejecutar comando remoto        |
| `view_logs`       | Ver logs                       |
| `debug_site`      | Toggle WP_DEBUG                |
| `cache_site`      | Gestionar cache headers        |
| `git_status`      | Estado Git remoto              |
| `set_domain`      | Cambiar dominio                |
| `redeploy`        | Forzar redeploy con health check |
| `setup_smtp`      | Configurar SMTP                |
| `minecraft`       | Gestionar servidores Minecraft |
| `coolify_failover` | Restaurar sitio en VPS alternativo |
| `coolify_restart`  | Reiniciar con only_db/only_wordpress |
| `coolify_switch_dns` | Conmutar DNS a otro target   |
| `install_coolify`  | Instalar Coolify en target nuevo |
| `deploy_websocket` | Desplegar WebSocket container  |
| `run_script`       | Ejecutar script remoto         |
| `schedule_backup`  | Programar backup automatizado  |

### Recursos MCP

| Recurso               | URI                   |
| --------------------- | --------------------- |
| Configuracion         | `coolify://config`    |
| Lista sitios          | `coolify://sites`     |
| Servidores Minecraft  | `coolify://minecraft` |
| Templates disponibles | `coolify://templates` |

## Configuracion

El archivo `config/settings.json` sigue el mismo formato que el coolify-manager PowerShell:

```json
{
    "vps": {
        "ip": "123.456.789.0",
        "user": "root",
        "sshKey": "C:/Users/user/.ssh/id_ed25519",
        "sshPassword": "opcional-si-no-hay-llave"
    },
    "coolify": {
        "baseUrl": "http://123.456.789.0:8000",
        "apiToken": "tu-token-coolify",
        "serverUuid": "srv-uuid",
        "projectUuid": "proj-uuid",
        "environmentName": "production"
    },
    "wordpress": {
        "dbUser": "manager",
        "dbPassword": "${DB_PASSWORD}",
        "defaultAdminEmail": "admin@example.com"
    },
    "glory": {
        "templateRepo": "https://github.com/user/template.git",
        "libraryRepo": "https://github.com/user/library.git",
        "defaultBranch": "main"
    },
    "sitios": [],
    "minecraft": []
}
```

Variables de entorno (`${VAR}`) se expanden automaticamente al cargar.

Para QM14 conviene mantener en `.env` las credenciales mutables: tokens de Coolify, SMTP, API Password de Contabo y datos de Google Drive.

### Configuracion extendida

`backupStorage.localDir` define dónde se guardan las copias fuera de la VPS. Si es relativa, se resuelve contra el directorio del config.

`backupStorage.remote` define el backend remoto para copias de seguridad. Soporta `sshremote` (recomendado, VPS secundario via SSH) y `googledrive` (legacy).

`smtp` configuracion SMTP global para alertas de salud y notificaciones.

`targets` permite definir destinos de migración con su propia VPS y su propio Coolify.

`dnsProviders` permite definir cuentas DNS/Contabo y `dnsConfig` por sitio habilita el failover controlado sin afectar Minecraft.

`backupPolicy` y `healthCheck` se pueden configurar por sitio.

```json
{
    "backupStorage": {
        "localDir": "backups",
        "remote": {
            "type": "sshremote",
            "host": "10.0.0.20",
            "user": "root",
            "sshPassword": "${VPS2_SSH_PASSWORD}",
            "baseDir": "/backups/coolify-manager"
        }
    },
    "smtp": {
        "host": "smtp-relay.brevo.com",
        "port": 587,
        "user": "alerts@example.com",
        "password": "${SMTP_PASSWORD}",
        "fromName": "Coolify Manager",
        "secure": "tls"
    },
    "dnsProviders": [
        {
            "name": "contabo-vps1",
            "type": "contabo",
            "clientId": "${CONTABO_VPS1_CLIENT_ID}",
            "clientSecret": "${CONTABO_VPS1_CLIENT_SECRET}",
            "username": "${CONTABO_VPS1_USERNAME}",
            "apiPassword": "${CONTABO_VPS1_API_PASSWORD}"
        }
    ],
    "targets": [
        {
            "name": "produccion-b",
            "vps": {
                "ip": "10.0.0.20",
                "user": "root",
                "sshKey": "C:/Users/user/.ssh/id_ed25519",
                "sshPassword": "opcional-si-no-hay-llave"
            },
            "coolify": {
                "baseUrl": "http://10.0.0.20:8000",
                "apiToken": "token-destino",
                "serverUuid": "srv-destino",
                "projectUuid": "proj-destino",
                "environmentName": "production"
            }
        }
    ],
    "sitios": [
        {
            "nombre": "blog",
            "dominio": "https://blog.com",
            "target": "produccion-b",
            "stackUuid": "abc123",
            "template": "wordpress",
            "dnsConfig": {
                "provider": "contabo-vps1",
                "zone": "blog.com",
                "switchOnMigration": true
            },
            "backupPolicy": {
                "enabled": true,
                "dailyKeep": 2,
                "weeklyKeep": 2,
                "sourcePaths": ["/var/www/html/wp-content"]
            },
            "healthCheck": {
                "httpPath": "/",
                "timeoutSeconds": 20,
                "fatalPatterns": ["Fatal error", "Uncaught Error"]
            }
        }
    ]
}
```

## Guia MCP para VS Code

Ver MCP-VSCODE.md para instalación, conexión y pruebas manuales en este workspace.

## Compatibilidad con el Manager Original

- Lee el mismo `settings.json` sin cambios.
- Despliega sobre los mismos stacks de Coolify.
- Los templates Docker Compose son identicos.
- Se puede usar en paralelo con el PowerShell original.

## Estructura del Proyecto

```
src/
  lib.rs               # Library entry point (re-exports modulos publicos)
  main.rs              # Binary entry point (CLI o MCP)
  api/                 # API estructurada (list_sites, health_check, etc.)
  cli/mod.rs           # Parser clap con 20+ subcomandos
  commands/            # 29 handlers de comandos (incl. failover, deploy-ws)
  mcp/                 # Servidor MCP (server, 26 tools, resources)
  services/            # Logica de negocio
  infra/               # SSH, API, Docker, templates, secrets
  config/mod.rs        # Carga y cache de settings.json
  domain/mod.rs        # Tipos de dominio (SiteConfig, etc.)
  error/mod.rs         # Tipos de error por capa
  logging/mod.rs       # Tracing dual mode (CLI: stdout+file, MCP: stderr/file)
gui/                   # Tauri v2 + React 19 desktop GUI
  src-tauri/           # Rust Tauri commands (workspace member)
  src/                 # React frontend (4 vistas, tema Kamples)
templates/             # Docker Compose YAML templates
config/                # settings.json (creado por usuario)
```
