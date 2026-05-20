# 🚀 coolify-manager-rs

> **Gestor completo de despliegues en Coolify** — CLI, servidor MCP para VS Code y GUI de escritorio, todo en un único binario Rust.

Administra sitios WordPress con tema Glory, servicios Docker Compose, backups automáticos, health checks, migraciones entre VPS, failover, auditorías de seguridad y mucho más. Reemplaza el coolify-manager PowerShell original con mayor velocidad, seguridad y funcionalidades.

---

## 📋 Contenido

- [Características](#-características)
- [Requisitos y build](#-requisitos-y-build)
- [Configuración](#-configuración)
- [Referencia de comandos CLI](#-referencia-de-comandos-cli)
- [Modo MCP (VS Code)](#-modo-mcp-vs-code)
- [GUI de escritorio](#-gui-de-escritorio)
- [Configuración avanzada](#-configuración-avanzada)
- [Arquitectura](#-arquitectura)
- [Tests](#-tests)
- [Compatibilidad](#-compatibilidad)

---

## ✨ Características

| Área | Capacidades |
|---|---|
| 🚢 **Deploy** | WordPress + Glory theme, servicios Rust/Docker Compose, zero-downtime, rollback automático |
| 🔒 **Seguridad** | Pre-deploy safety check, backup automático previo, fix-db-auth, wp-security |
| 💾 **Backups** | SSH remoto (VPS secundario), Google Drive legacy, tiers daily/weekly/manual, restore validado |
| 🏥 **Health** | HTTP check, patrones fatales, alertas SMTP, `--all` en un solo comando, autorepair |
| 🔄 **Migración** | Migración completa entre targets, dry-run con preflight real, conmutación DNS automática |
| 🚨 **Failover** | Restaura un sitio en VPS alternativo sin necesitar el VPS origen |
| 📊 **Auditoría** | Rendimiento VPS, seguridad WordPress, rotación de contraseñas admin |
| 🔌 **MCP** | 26+ herramientas para GitHub Copilot / VS Code (JSON-RPC 2.0 sobre stdio) |
| 🖥️ **GUI** | React 19 + Tauri v2 opcional, también usable como app web con API local |
| 🎮 **Extras** | Servidores Minecraft, WebSocket Bun, sincronización de variables de entorno |

---

## ⚙️ Requisitos y build

### Requisitos

- **Rust 1.70+** (probado con 1.94) — [instalar rustup](https://rustup.rs/)
- **Cargo** (incluido con rustup)
- Acceso SSH al VPS y token de la API de Coolify

### Build

```bash
cargo build --release
```

El binario se genera en `target/release/coolify-manager.exe` (~10 MB, sin dependencias externas).

### GUI de desarrollo

```bash
# Modo Tauri nativo
npm run dev

# Modo web (API local + Vite, sin Tauri)
npm run dev:web
```

`dev:web` arranca `cargo run -- gui-api --bind 127.0.0.1:8787` y Vite en paralelo.
Modo demo forzado: `VITE_COOLIFY_MANAGER_DEMO=1`.

---

## 🔧 Configuración

### Variables de entorno

El binario carga `.env` y `.env.local` automáticamente desde la raíz del proyecto, antes de leer `config/settings.json`. Las variables se expanden en el JSON con la sintaxis `${VAR}`.

Usa `.env.example` como plantilla. El archivo real `.env` está en `.gitignore`.

Para ver qué ruta de configuración está usando el binario:

```bash
coolify-manager get-config-path
```

> La ruta se resuelve en este orden: `--config` → `COOLIFY_MANAGER_CONFIG` → ancestros del CWD → `CARGO_MANIFEST_DIR` → ancestros del ejecutable.

### `config/settings.json` — estructura mínima

```json
{
    "vps": {
        "ip": "123.456.789.0",
        "user": "root",
        "sshKey": "C:/Users/user/.ssh/id_ed25519"
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
    "sitios": []
}
```

---

## 📖 Referencia de comandos CLI

```
coolify-manager [OPCIONES GLOBALES] <SUBCOMANDO> [OPCIONES]
```

### Opciones globales

| Opción | Descripción |
|---|---|
| `--log-level <NIVEL>` | Nivel de logging: `trace`, `debug`, `info`, `warn`, `error` (por defecto: `info`) |
| `--log-dir <DIR>` | Directorio para archivos de log |
| `-c, --config <RUTA>` | Ruta al `settings.json` |
| `--mcp` | Inicia en modo MCP (servidor stdio JSON-RPC 2.0) |
| `--help` | Muestra ayuda |
| `--version` | Muestra versión |

---

### 🏗️ Creación y despliegue

#### `new` — Crear sitio WordPress

```bash
coolify-manager new --name mi-sitio --domain https://mi-sitio.com
coolify-manager new --name mi-sitio --domain https://mi-sitio.com --template wordpress --target produccion-b
```

| Opción | Descripción |
|---|---|
| `-n, --name` | Nombre único del sitio (slug) |
| `-d, --domain` | Dominio completo con protocolo (`https://...`) |
| `--template` | Template de stack: `wordpress`, `kamples`, `minecraft` (por defecto: `wordpress`) |
| `--target` | Target donde desplegar (definido en `settings.json`) |
| `--skip-theme` | Omite instalación del tema Glory |
| `--skip-cache` | Omite configuración de cache headers |

---

#### `deploy` — Desplegar / actualizar tema Glory

```bash
coolify-manager deploy --name mi-sitio
coolify-manager deploy --name mi-sitio --update --skip-backup
coolify-manager deploy --name mi-sitio --force --glory-branch feature/nueva-ui
```

| Opción | Descripción |
|---|---|
| `-n, --name` | Nombre del sitio |
| `--update` | Actualiza en vez de reinstalar |
| `--glory-branch` | Rama del tema Glory |
| `--library-branch` | Rama de la librería Glory |
| `--skip-react` | Omite compilación de React |
| `--force` | Fuerza `git reset --hard` antes del pull |
| `--skip-backup` | Omite backup automático pre-deploy |

> ⚠️ Cada deploy ejecuta un pre-deploy safety check (verifica todos los sitios en Coolify) y crea un backup automático antes de aplicar cambios. Usa `--skip-backup` para omitirlo en cambios de bajo riesgo.

---

#### `deploy-service` — Deploy zero-downtime para servicios Docker Compose

Para servicios Rust u otros contenedores personalizados. Construye imagen nueva y hace swap sin downtime.

```bash
coolify-manager deploy-service --name mi-servicio
coolify-manager deploy-service --name mi-servicio --skip-build --seed
```

| Opción | Descripción |
|---|---|
| `-n, --name` | Nombre del sitio en `settings.json` |
| `--skip-build` | Omite build (asume imagen ya construida) |
| `--seed` | Ejecuta seed de datos de prueba post-deploy |
| `--skip-compose-sync` | No sincroniza el compose con la API de Coolify |
| `--skip-backup` | Omite backup pre-deploy |

---

#### `redeploy` — Redeploy seguro del servicio

```bash
coolify-manager redeploy --name mi-sitio
coolify-manager redeploy --name mi-sitio --skip-backup
```

> En stacks Rust, `redeploy` usa el mismo flujo protegido que `deploy-service` (build + swap sin stop/start). En WordPress, dispara el mecanismo de redeploy de Coolify.

---

### 📋 Operaciones de sitio

#### `list` — Listar sitios configurados

```bash
coolify-manager list
coolify-manager list --detailed
```

---

#### `restart` — Reiniciar servicios

```bash
coolify-manager restart --name mi-sitio
coolify-manager restart --all
coolify-manager restart --name mi-sitio --only-db
coolify-manager restart --name mi-sitio --only-wordpress
```

| Opción | Descripción |
|---|---|
| `-n, --name` | Nombre del sitio |
| `--all` | Reinicia todos los sitios |
| `--only-db` | Solo reinicia el contenedor de base de datos |
| `--only-wordpress` | Solo reinicia el contenedor WordPress |

---

#### `logs` — Ver logs del contenedor

```bash
coolify-manager logs --name mi-sitio
coolify-manager logs --name mi-sitio --lines 100 --target wordpress
coolify-manager logs --name mi-sitio --wp-debug --filter "Fatal error"
```

| Opción | Descripción |
|---|---|
| `-n, --name` | Nombre del sitio |
| `-l, --lines` | Número de líneas (por defecto: 50) |
| `--target` | Contenedor objetivo: `wordpress`, `mariadb`, `postgres` |
| `--wp-debug` | Ver `debug.log` de WordPress en vez de container logs |
| `--filter` | Filtrar salida por patrón |

---

#### `exec` — Ejecutar comando en el contenedor

```bash
coolify-manager exec --name mi-sitio --command "wp option get siteurl"
coolify-manager exec --name mi-sitio --php "echo get_option('blogname');"
coolify-manager exec --name mi-sitio --command "psql -U user -c '\dt'" --target postgres
```

| Opción | Descripción |
|---|---|
| `-n, --name` | Nombre del sitio |
| `--command` | Comando bash a ejecutar |
| `--php` | Código PHP a ejecutar (vía `wp eval`) |
| `--target` | Contenedor objetivo: `wordpress` (por defecto), `mariadb`, `postgres` |

---

#### `run-script` — Subir y ejecutar un script local

```bash
coolify-manager run-script --name mi-sitio --file ./scripts/fix-perms.sh
coolify-manager run-script --name mi-sitio --file ./fix.php --interpreter php
```

| Opción | Descripción |
|---|---|
| `-n, --name` | Nombre del sitio |
| `-f, --file` | Ruta al script local |
| `-i, --interpreter` | Intérprete: `php`, `bash`, `python3` (auto-detecta por extensión) |
| `--target` | Contenedor objetivo (por defecto: `wordpress`) |
| `--args` | Argumentos adicionales para el script |

---

#### `debug` — Activar / desactivar WP_DEBUG

```bash
coolify-manager debug --name mi-sitio --enable
coolify-manager debug --name mi-sitio --disable
coolify-manager debug --name mi-sitio --status
```

---

#### `set-domain` — Cambiar dominio del sitio

```bash
coolify-manager set-domain --name mi-sitio --domain https://nuevo-dominio.com
```

---

#### `git-status` — Estado Git del tema remoto

```bash
coolify-manager git-status --name mi-sitio
```

---

### 💾 Base de datos

#### `import` — Importar SQL

```bash
coolify-manager import --name mi-sitio --file backup.sql
coolify-manager import --name mi-sitio --file backup.sql --fix-urls
```

| Opción | Descripción |
|---|---|
| `-n, --name` | Nombre del sitio |
| `-f, --file` | Ruta local al archivo `.sql` |
| `--fix-urls` | Corrige URLs al dominio configurado tras importar |

---

#### `export` — Exportar SQL

```bash
coolify-manager export --name mi-sitio
coolify-manager export --name mi-sitio --output ./backups/export.sql
```

---

#### `fix-db-auth` — Corregir mismatch de contraseña BD

Detecta y corrige cuando `DATABASE_URL` y la contraseña real de PostgreSQL no coinciden. Causa frecuente: colisión del hostname `postgres` en redes Docker multi-stack.

```bash
coolify-manager fix-db-auth --name mi-sitio
coolify-manager fix-db-auth --name mi-sitio --dry-run
```

---

### 🔄 Variables de entorno

#### `sync-env` — Sincronizar .env con Coolify

```bash
# Solo mostrar diferencias
coolify-manager sync-env --name mi-sitio --direction diff

# Subir variables locales a Coolify
coolify-manager sync-env --name mi-sitio --direction push

# Bajar variables de Coolify al .env local
coolify-manager sync-env --name mi-sitio --direction pull

# Solo sincronizar claves específicas
coolify-manager sync-env --name mi-sitio --direction push --only APP_KEY,STRIPE_SECRET

# Ver qué haría sin aplicar cambios
coolify-manager sync-env --name mi-sitio --direction push --dry-run
```

| Opción | Descripción |
|---|---|
| `-n, --name` | Nombre del sitio |
| `--direction` | `diff` (solo mostrar), `push` (local→Coolify), `pull` (Coolify→local) |
| `--dry-run` | Muestra diferencias sin aplicar |
| `--env-file` | Ruta al `.env` local (auto-detecta por defecto) |
| `--only` | Limita a claves específicas (separadas por coma) |

---

### 🏥 Salud y seguridad

#### `health` — Health checks

```bash
coolify-manager health --name mi-sitio
coolify-manager health --all
coolify-manager health --all --alert
coolify-manager health --name mi-sitio --repair
```

| Opción | Descripción |
|---|---|
| `-n, --name` | Nombre del sitio (requerido sin `--all`) |
| `--all` | Verifica todos los sitios |
| `--alert` | Envía email SMTP si algún sitio está caído |
| `--repair` | Repara fallos recuperables de red en servicios Rust |

---

#### `wp-security` — Auditoría de seguridad WordPress

```bash
# Auditar plugins, permisos, configuraciones inseguras
coolify-manager wp-security --name mi-sitio --audit

# Rotar contraseña del admin (genera una aleatoria si no se especifica)
coolify-manager wp-security --name mi-sitio --user admin
coolify-manager wp-security --name mi-sitio --user admin --password nueva-pass-segura
```

---

#### `cache` — Gestionar cache headers HTTP

```bash
coolify-manager cache --name mi-sitio --action enable
coolify-manager cache --name mi-sitio --action disable
coolify-manager cache --name mi-sitio --action status
coolify-manager cache --action enable --all
```

---

### 📧 SMTP

#### `smtp` — Configurar relay de correo

```bash
coolify-manager smtp --name mi-sitio --host smtp-relay.brevo.com --port 587
coolify-manager smtp --all
coolify-manager smtp --name mi-sitio --test --test-email admin@ejemplo.com
coolify-manager smtp --name mi-sitio --status
```

---

### 💾 Backups y restauración

#### `backup` — Crear o listar backups externos

```bash
# Backup manual con etiqueta
coolify-manager backup --name mi-sitio --tier manual --label antes-de-update

# Backup diario (para usar en cron)
coolify-manager backup --name mi-sitio --tier daily

# Listar backups disponibles
coolify-manager backup --name mi-sitio --list
```

| Opción | Descripción |
|---|---|
| `-n, --name` | Nombre del sitio |
| `--tier` | Tier: `daily`, `weekly`, `manual` (por defecto: `manual`) |
| `--label` | Etiqueta descriptiva opcional |
| `--list` | Lista backups en vez de crear uno nuevo |

> Los backups se almacenan en el backend configurado: SSH a VPS secundario (`sshremote`, recomendado) o Google Drive (legacy). Estructura remota: `{baseDir}/{sitio}/{tier}/{backup_id}.tar.gz`.

---

#### `restore` — Restaurar backup

```bash
coolify-manager restore --name mi-sitio --backup-id 20260314_120000-antes_de_update
coolify-manager restore --name mi-sitio --backup-id 20260314_120000 --skip-safety-snapshot
```

| Opción | Descripción |
|---|---|
| `-n, --name` | Nombre del sitio |
| `--backup-id` | Identificador del backup |
| `--skip-safety-snapshot` | Omite snapshot de seguridad previo a la restauración |

---

#### `schedule-backup` — Programar backups automáticos (Windows Task Scheduler)

```bash
# Crear tarea programada para un sitio
coolify-manager schedule-backup --name mi-sitio

# Crear tareas para todos los sitios habilitados
coolify-manager schedule-backup

# Eliminar tareas programadas
coolify-manager schedule-backup --name mi-sitio --remove
```

---

#### `auth-drive` — Autorizar Google Drive (backend legacy)

```bash
coolify-manager auth-drive
```

Inicia el flujo OAuth para autorizar el acceso a Google Drive.

---

### 🌐 Migración y DNS

#### `migrate` — Migrar sitio a otro target

```bash
# Validar el plan sin ejecutar (preflight real)
coolify-manager migrate --name mi-sitio --target produccion-b --dry-run

# Migración completa con conmutación DNS automática
coolify-manager migrate --name mi-sitio --target standby-vps2 --switch-dns
```

| Opción | Descripción |
|---|---|
| `-n, --name` | Nombre del sitio |
| `--target` | Nombre del target en `settings.json` |
| `--dry-run` | Preflight real: valida origen/destino sin copiar datos |
| `--switch-dns` | Conmuta DNS al target tras health OK |

---

#### `switch-dns` — Conmutar DNS manualmente

```bash
# Conmutar al DNS de un target configurado
coolify-manager switch-dns --name mi-sitio --target standby-vps2 --dry-run
coolify-manager switch-dns --name mi-sitio --target standby-vps2

# Conmutar a una IP explícita
coolify-manager switch-dns --name mi-sitio --ip 173.249.50.44
```

---

#### `failover` — Restaurar sitio en VPS alternativo

Recupera un sitio completo en otro VPS usando el backup más reciente, sin necesitar que el VPS original esté operativo.

```bash
coolify-manager failover --name mi-sitio --target standby-vps2
coolify-manager failover --name mi-sitio --target standby-vps2 --switch-dns
coolify-manager failover --name mi-sitio --target standby-vps2 --backup-id 20260314_120000
```

| Opción | Descripción |
|---|---|
| `-n, --name` | Nombre del sitio |
| `--target` | Target destino en `settings.json` |
| `--backup-id` | ID de backup específico (por defecto: el más reciente) |
| `--switch-dns` | Conmuta DNS al target tras health OK |
| `--skip-provision` | Usa el `stackUuid` existente sin provisionar uno nuevo |

---

### 🔍 Auditoría e infraestructura

#### `audit` — Auditar VPS

```bash
coolify-manager audit
coolify-manager audit --target produccion-b
```

Verifica rendimiento (CPU, RAM, disco), configuración de seguridad, versiones y servicios del VPS.

---

#### `install-coolify` — Instalar Coolify en VPS remoto

```bash
coolify-manager install-coolify --target standby-vps2
```

Conecta por SSH al target e instala Coolify automáticamente.

---

#### `deploy-websocket` — Agregar servicio WebSocket

Agrega un servicio WebSocket (Bun) a un stack Kamples existente.

```bash
coolify-manager deploy-websocket --name mi-sitio-kamples
```

---

### 🎮 Minecraft

```bash
# Crear servidor
coolify-manager minecraft --action new --server-name survival --memory 4G --max-players 20

# Ver logs
coolify-manager minecraft --action logs --server-name survival --lines 100

# Reiniciar
coolify-manager minecraft --action restart --server-name survival

# Ejecutar comando de consola
coolify-manager minecraft --action console --server-name survival --console-command "say Hola"

# Estado del servidor
coolify-manager minecraft --action status --server-name survival

# Eliminar servidor
coolify-manager minecraft --action remove --server-name survival
```

| Opción | Descripción |
|---|---|
| `-a, --action` | `new`, `logs`, `console`, `restart`, `status`, `remove` |
| `-s, --server-name` | Nombre del servidor |
| `--memory` | RAM asignada (por defecto: `2G`) |
| `--max-players` | Máximo de jugadores (por defecto: `20`) |
| `--version` | Versión de Minecraft (por defecto: `LATEST`) |
| `--difficulty` | Dificultad: `peaceful`, `easy`, `normal`, `hard` |
| `--port` | Puerto externo (por defecto: `25565`) |

---

### 🛠️ Utilidades

#### `get-config-path` — Ver ruta de settings.json

```bash
coolify-manager get-config-path
```

#### `gui-api` — Iniciar API HTTP local para la GUI web

```bash
coolify-manager gui-api
coolify-manager gui-api --bind 127.0.0.1:8787
```

---

## 🔌 Modo MCP (VS Code)

El servidor MCP expone las capacidades del manager como herramientas para GitHub Copilot y otros clientes compatibles con el protocolo MCP.

### Iniciar en modo MCP

```bash
coolify-manager --mcp
```

> Si se invoca sin subcomando, el binario entra en modo MCP automáticamente.

### Configuración en `.vscode/mcp.json`

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

### 🧰 Herramientas MCP disponibles

| Herramienta | Descripción |
|---|---|
| `new_site` | Crear sitio WordPress con tema Glory |
| `deploy_theme` | Desplegar o actualizar tema Glory |
| `list_sites` | Listar sitios configurados |
| `restart_site` | Reiniciar servicios del sitio |
| `import_database` | Importar SQL a la base de datos |
| `export_database` | Exportar base de datos a SQL |
| `coolify_backup` | Crear o listar backups externos |
| `coolify_restore_backup` | Restaurar backup validado |
| `coolify_health` | Health checks (individual o todos + alerta) |
| `coolify_migrate` | Migrar sitio completo a otro target |
| `coolify_audit_vps` | Auditar VPS principal o target |
| `coolify_wp_security` | Auditar WordPress y rotar contraseña admin |
| `exec_command` | Ejecutar comando en contenedor |
| `view_logs` | Ver logs del contenedor |
| `debug_site` | Activar/desactivar WP_DEBUG |
| `cache_site` | Gestionar cache headers HTTP |
| `git_status` | Estado Git del tema remoto |
| `set_domain` | Cambiar dominio del sitio |
| `redeploy` | Redeploy seguro (WP vía Coolify API, Rust vía build+swap) |
| `setup_smtp` | Configurar SMTP relay |
| `minecraft` | Gestionar servidores Minecraft |
| `coolify_failover` | Restaurar sitio en VPS alternativo |
| `coolify_restart` | Reiniciar con `only_db` / `only_wordpress` |
| `coolify_switch_dns` | Conmutar DNS a otro target o IP |
| `install_coolify` | Instalar Coolify en target remoto |
| `deploy_websocket` | Desplegar servicio WebSocket (Bun) |
| `run_script` | Subir y ejecutar script local en contenedor |
| `schedule_backup` | Programar backup automático |

### 📦 Recursos MCP

| URI | Contenido |
|---|---|
| `coolify://config` | Configuración actual |
| `coolify://sites` | Lista de sitios con estado |
| `coolify://minecraft` | Servidores Minecraft |
| `coolify://templates` | Templates Docker Compose disponibles |

---

## 🖥️ GUI de escritorio

Interfaz visual en React 19 + Tauri v2 con:

- 📊 Dashboard VPS: CPU, RAM, disco en tiempo real
- 🟢 Tabla de sitios con estado inline (activo, degradado, caído)
- ⚡ Acciones contextuales: deploy, restart, backup, logs
- 💾 Backups globales y por sitio
- 🖊️ Consola de comandos integrada

### Modos de uso

| Modo | Comando | Descripción |
|---|---|---|
| Tauri nativo | `npm run dev` | App de escritorio completa |
| Web con API local | `npm run dev:web` | Navegador + API HTTP en `localhost:8787` |
| Producción web | `coolify-manager gui-api` + build estático | Sin Tauri |

---

## 🔧 Configuración avanzada

### Backups remotos (SSH recomendado)

```json
{
    "backupStorage": {
        "localDir": "backups",
        "remote": {
            "type": "sshremote",
            "host": "10.0.0.20",
            "user": "root",
            "sshKey": "C:/Users/user/.ssh/id_ed25519",
            "baseDir": "/backups/coolify-manager"
        }
    }
}
```

> `type` soporta `sshremote` (recomendado) y `googledrive` (legacy).
> La integridad de cada backup se verifica post-upload comparando tamaños.

---

### Alertas SMTP

```json
{
    "smtp": {
        "host": "smtp-relay.brevo.com",
        "port": 587,
        "user": "alertas@ejemplo.com",
        "password": "${SMTP_PASSWORD}",
        "fromName": "Coolify Manager",
        "secure": "tls"
    }
}
```

---

### Targets múltiples (migración / failover)

```json
{
    "targets": [
        {
            "name": "standby-vps2",
            "vps": {
                "ip": "10.0.0.20",
                "user": "root",
                "sshKey": "C:/Users/user/.ssh/id_ed25519"
            },
            "coolify": {
                "baseUrl": "http://10.0.0.20:8000",
                "apiToken": "${VPS2_COOLIFY_TOKEN}",
                "serverUuid": "srv-destino",
                "projectUuid": "proj-destino",
                "environmentName": "production"
            }
        }
    ]
}
```

---

### DNS automático (Contabo)

```json
{
    "dnsProviders": [
        {
            "name": "contabo-vps1",
            "type": "contabo",
            "clientId": "${CONTABO_CLIENT_ID}",
            "clientSecret": "${CONTABO_CLIENT_SECRET}",
            "username": "${CONTABO_USERNAME}",
            "apiPassword": "${CONTABO_API_PASSWORD}"
        }
    ]
}
```

---

### Configuración por sitio

```json
{
    "sitios": [
        {
            "nombre": "blog",
            "dominio": "https://blog.com",
            "target": "standby-vps2",
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

---

## 🏛️ Arquitectura

```
                      ┌── CLI (clap · 33 subcomandos)
coolify_manager ──▶   ├── MCP (JSON-RPC 2.0 sobre stdio · 26 tools)
   (librería)         └── GUI API (HTTP local · Tauri v2 / navegador)
        │
        ▼
   Commands (33 handlers)
        │
        ▼
   Services (lógica de negocio)
   ├── deploy / rollback / theme
   ├── backup / restore / schedule
   ├── health / alert
   ├── migrate / failover / dns
   ├── audit / security
   └── minecraft / websocket
        │
        ▼
   Infrastructure
   ├── SSH nativo (russh)
   ├── Coolify API (reqwest + rustls)
   ├── Docker Compose
   ├── Templates YAML
   └── Secrets / env
```

**Dual target:** `lib.rs` (librería) + `main.rs` (binario). La GUI, el MCP y los tests consumen la misma API interna.

### Estructura de directorios

```
src/
  lib.rs               # Punto de entrada de la librería
  main.rs              # Punto de entrada del binario (CLI / MCP / GUI API)
  api/                 # Funciones estructuradas (SiteSummary, HealthResponse…)
  cli/mod.rs           # Parser clap con 33 subcomandos
  commands/            # Handlers individuales por subcomando
  mcp/                 # Servidor MCP (26 tools + resources)
  services/            # Lógica de negocio por dominio
  infra/               # SSH, API Coolify, Docker, templates, secrets
  config/mod.rs        # Carga y caché de settings.json
  domain/mod.rs        # Tipos de dominio (SiteConfig, BackupMeta…)
  error/mod.rs         # Tipos de error por capa
  logging/mod.rs       # Tracing dual (CLI: stdout+archivo / MCP: stderr/archivo)
gui/
  src/                 # React 19 frontend
  src-tauri/           # Comandos Tauri v2 (workspace member Cargo)
templates/             # Templates Docker Compose YAML
config/                # settings.json (no versionado, en .gitignore)
scripts/               # Scripts auxiliares
```

---

## 🧪 Tests

```bash
cargo test
```

91 tests unitarios cubriendo: configuración, validación, templates, rollback, domain types, errores, secrets, carga de entorno, SSH encoding, Google Drive, SSH backup, utilidades del sistema de backup y API.

---

## 🔁 Compatibilidad

- ✅ Lee el mismo `config/settings.json` del coolify-manager PowerShell original sin cambios.
- ✅ Compatible con los stacks Docker Compose existentes en Coolify.
- ✅ Los templates Docker Compose son idénticos a los del manager original.
- ✅ Se puede usar en paralelo con el PowerShell mientras se migra.

---

## 📄 Licencia

MIT — ver [LICENSE](./LICENSE)
