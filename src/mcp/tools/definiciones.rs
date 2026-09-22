/*
 * MCP Tools — herramientas disponibles via MCP.
 * Cada tool mapea a un comando CLI con el mismo handler.
 * [257B-1] Actualizado coolify_view_logs con parámetros since/until/pattern.
 */

use serde_json::Value;

/// Retorna la definicion de todas las tools MCP.
pub fn list_tools() -> Vec<Value> {
    let mut todas = definiciones_nucleo();
    todas.extend(definiciones_diagnostico());
    todas.extend(definiciones_gestion());
    todas
}

/* [119A-3] Definiciones por bloque secuencial (orden MCP estable, sin reordenar). */
fn definiciones_nucleo() -> Vec<Value> {
    let mut todas = definiciones_nucleo_sitios();
    todas.extend(definiciones_nucleo_bd());
    todas
}

/* Tools de sitio: crear, desplegar tema, listar y reiniciar. */
fn definiciones_nucleo_sitios() -> Vec<Value> {
    vec![
        tool_def(
            "coolify_new_site",
            "Crea un nuevo sitio WordPress con tema Glory en Coolify",
            serde_json::json!({
                "type": "object",
                "required": ["site_name", "domain"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre unico del sitio (slug)" },
                    "domain": { "type": "string", "description": "Dominio completo con protocolo (https://...)" },
                    "glory_branch": { "type": "string", "description": "Rama del tema Glory", "default": "main" },
                    "library_branch": { "type": "string", "description": "Rama de la libreria Glory", "default": "main" },
                    "template": { "type": "string", "description": "Template de stack", "default": "wordpress", "enum": ["wordpress", "kamples", "minecraft"] },
                    "target": { "type": "string", "description": "Target opcional definido en settings.json" },
                    "image": { "type": "string", "description": "[119A-4] Imagen precompilada registry/owner/app:tag para stacks Rust (pull en vez de build; exige tag fijo, sin latest)" },
                    "skip_theme": { "type": "boolean", "description": "Omitir instalacion del tema", "default": false },
                    "skip_cache": { "type": "boolean", "description": "Omitir cache headers", "default": false }
                }
            }),
        ),
        tool_def(
            "coolify_deploy_theme",
            "Despliega o actualiza el tema Glory en un sitio existente",
            serde_json::json!({
                "type": "object",
                "required": ["site_name"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "glory_branch": { "type": "string", "description": "Rama del tema Glory" },
                    "library_branch": { "type": "string", "description": "Rama de la libreria Glory" },
                    "update": { "type": "boolean", "description": "Actualizar en vez de reinstalar", "default": false },
                    "skip_react": { "type": "boolean", "description": "Omitir build de React", "default": false },
                    "force": { "type": "boolean", "description": "Forzar git reset --hard", "default": false }
                }
            }),
        ),
        tool_def(
            "coolify_list_sites",
            "Lista todos los sitios configurados con su estado",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "detailed": { "type": "boolean", "description": "Informacion detallada", "default": false }
                }
            }),
        ),
        tool_def(
            "coolify_restart",
            "Reinicia los servicios de un sitio",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "all": { "type": "boolean", "description": "Reiniciar todos", "default": false },
                    "only_db": { "type": "boolean", "description": "Solo reiniciar contenedor de BD", "default": false },
                    "only_wordpress": { "type": "boolean", "description": "Solo reiniciar contenedor WordPress", "default": false }
                }
            }),
        ),
    ]
}

/* Tools de BD: importar, exportar, backup y restore. */
fn definiciones_nucleo_bd() -> Vec<Value> {
    vec![
        tool_def(
            "coolify_import_db",
            "Importa un archivo SQL en la base de datos del sitio",
            serde_json::json!({
                "type": "object",
                "required": ["site_name", "sql_file_path"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "sql_file_path": { "type": "string", "description": "Ruta al archivo .sql" },
                    "fix_urls": { "type": "boolean", "description": "Corregir URLs tras importar", "default": false }
                }
            }),
        ),
        tool_def(
            "coolify_export_db",
            "Exporta la base de datos a un archivo SQL",
            serde_json::json!({
                "type": "object",
                "required": ["site_name"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "output_path": { "type": "string", "description": "Ruta de salida" }
                }
            }),
        ),
        tool_def(
            "coolify_backup",
            "Crea o lista backups externos validados de un sitio",
            serde_json::json!({
                "type": "object",
                "required": ["site_name"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "tier": { "type": "string", "description": "Tier del backup", "default": "manual", "enum": ["daily", "weekly", "manual"] },
                    "label": { "type": "string", "description": "Etiqueta opcional" },
                    "list": { "type": "boolean", "description": "Lista backups existentes", "default": false }
                }
            }),
        ),
        tool_def(
            "coolify_restore_backup",
            "Restaura un backup valido en un sitio",
            serde_json::json!({
                "type": "object",
                "required": ["site_name", "backup_id"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "backup_id": { "type": "string", "description": "Identificador del backup" },
                    "skip_safety_snapshot": { "type": "boolean", "description": "Omitir snapshot previo", "default": false }
                }
            }),
        ),
    ]
}

fn definiciones_diagnostico() -> Vec<Value> {
    let mut todas = definiciones_diagnostico_salud();
    todas.extend(definiciones_diagnostico_runtime());
    todas
}

/* Diagnostico de salud: health, migrate, DNS, auditoria y seguridad WP. */
fn definiciones_diagnostico_salud() -> Vec<Value> {
    vec![
        tool_def(
            "coolify_health",
            "Ejecuta health checks del sitio",
            serde_json::json!({
                "type": "object",
                "required": ["site_name"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "alert": { "type": "boolean", "description": "Enviar alerta si el sitio esta caido", "default": false },
                    "repair": { "type": "boolean", "description": "Reparar fallos recuperables de red en servicios Rust", "default": false }
                }
            }),
        ),
        tool_def(
            "coolify_migrate",
            "Migra un sitio completo a otro target configurado",
            serde_json::json!({
                "type": "object",
                "required": ["site_name", "target"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "target": { "type": "string", "description": "Target definido en settings.json" },
                    "dry_run": { "type": "boolean", "description": "Genera plan sin ejecutar", "default": false },
                    "switch_dns": { "type": "boolean", "description": "Conmuta DNS al target tras health OK", "default": false }
                }
            }),
        ),
        tool_def(
            "coolify_switch_dns",
            "Conmuta los registros DNS del sitio hacia una IP o target",
            serde_json::json!({
                "type": "object",
                "required": ["site_name"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "target": { "type": "string", "description": "Target definido en settings.json" },
                    "target_ip": { "type": "string", "description": "IP explícita destino" },
                    "dry_run": { "type": "boolean", "description": "Solo muestra acciones", "default": false }
                }
            }),
        ),
        tool_def(
            "coolify_audit_vps",
            "Audita rendimiento y seguridad de una VPS o target",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "target": { "type": "string", "description": "Target opcional; si se omite usa la VPS principal" }
                }
            }),
        ),
        tool_def(
            "coolify_wp_security",
            "Audita WordPress y permite rotar password admin",
            serde_json::json!({
                "type": "object",
                "required": ["site_name"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "audit": { "type": "boolean", "description": "Ejecutar auditoría", "default": true },
                    "user": { "type": "string", "description": "Usuario admin a rotar" },
                    "password": { "type": "string", "description": "Nueva password; si se omite se genera" }
                }
            }),
        ),
    ]
}

/* Diagnostico runtime: exec, logs, debug, cache y git status. */
fn definiciones_diagnostico_runtime() -> Vec<Value> {
    vec![
        tool_def(
            "coolify_exec",
            "Ejecuta un comando dentro del contenedor del sitio",
            serde_json::json!({
                "type": "object",
                "required": ["site_name"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "command": { "type": "string", "description": "Comando bash" },
                    "php_code": { "type": "string", "description": "Codigo PHP" },
                    "target": { "type": "string", "description": "Contenedor objetivo", "default": "wordpress", "enum": ["wordpress", "mariadb"] }
                }
            }),
        ),
        tool_def(
            "coolify_view_logs",
            "Obtiene logs del contenedor o debug.log",
            serde_json::json!({
                "type": "object",
                "required": ["site_name"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "lines": { "type": "integer", "description": "Lineas a mostrar", "default": 50 },
                    "target": { "type": "string", "description": "Contenedor", "default": "wordpress" },
                    "wp_debug": { "type": "boolean", "description": "Ver debug.log", "default": false },
                    "filter": { "type": "string", "description": "Filtrar por patron" },
                    "docker_socket": { "type": "string", "description": "Docker socket endpoint" },
                    "since": { "type": "string", "description": "Solo logs desde este tiempo (ej: 2h, 24h, 2d)" },
                    "until": { "type": "string", "description": "Solo logs hasta este tiempo" },
                    "pattern": { "type": "string", "description": "Filtrar por múltiples patrones" }
                }
            }),
        ),
        tool_def(
            "coolify_debug",
            "Gestiona WP_DEBUG en un sitio",
            serde_json::json!({
                "type": "object",
                "required": ["site_name"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "enable": { "type": "boolean", "description": "Habilitar WP_DEBUG" },
                    "disable": { "type": "boolean", "description": "Deshabilitar WP_DEBUG" }
                }
            }),
        ),
        tool_def(
            "coolify_cache",
            "Gestiona cache headers HTTP del sitio",
            serde_json::json!({
                "type": "object",
                "required": ["action"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "action": { "type": "string", "description": "Accion", "enum": ["status", "enable", "disable"] },
                    "all": { "type": "boolean", "description": "Aplicar a todos", "default": false }
                }
            }),
        ),
        tool_def(
            "coolify_git_status",
            "Estado de Git en el tema Glory remoto",
            serde_json::json!({
                "type": "object",
                "required": ["site_name"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" }
                }
            }),
        ),
    ]
}

fn definiciones_gestion() -> Vec<Value> {
    let mut todas = definiciones_gestion_sitios();
    todas.extend(definiciones_gestion_ops());
    todas
}

/* Gestion de sitios: dominio, redeploy, SMTP, minecraft y failover. */
fn definiciones_gestion_sitios() -> Vec<Value> {
    vec![
        tool_def(
            "coolify_set_domain",
            "Cambia el dominio de un sitio WordPress",
            serde_json::json!({
                "type": "object",
                "required": ["site_name", "new_domain"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "new_domain": { "type": "string", "description": "Nuevo dominio con https://" }
                }
            }),
        ),
        tool_def(
            "coolify_redeploy",
            "Fuerza un redeploy del servicio",
            serde_json::json!({
                "type": "object",
                "required": ["site_name"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" }
                }
            }),
        ),
        tool_def(
            "coolify_setup_smtp",
            "Configura SMTP relay en el sitio",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "all": { "type": "boolean", "description": "Todos los sitios", "default": false },
                    "test": { "type": "boolean", "description": "Enviar email de prueba", "default": false },
                    "test_email": { "type": "string", "description": "Email destino para prueba" }
                }
            }),
        ),
        tool_def(
            "coolify_minecraft",
            "Gestiona servidores Minecraft",
            serde_json::json!({
                "type": "object",
                "required": ["action", "server_name"],
                "properties": {
                    "action": { "type": "string", "description": "Accion", "enum": ["new", "logs", "console", "restart", "status", "remove"] },
                    "server_name": { "type": "string", "description": "Nombre del servidor" },
                    "memory": { "type": "string", "description": "RAM", "default": "2G" },
                    "max_players": { "type": "integer", "description": "Max jugadores", "default": 20 },
                    "difficulty": { "type": "string", "description": "Dificultad", "default": "normal" },
                    "console_command": { "type": "string", "description": "Comando MC (para action=console)" },
                    "lines": { "type": "integer", "description": "Lineas de log", "default": 100 }
                }
            }),
        ),
        tool_def(
            "coolify_failover",
            "Failover: restaura un sitio en VPS alternativo usando backup de Drive",
            serde_json::json!({
                "type": "object",
                "required": ["site_name", "target"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "target": { "type": "string", "description": "Target destino definido en settings.json" },
                    "backup_id": { "type": "string", "description": "ID de backup especifico" },
                    "switch_dns": { "type": "boolean", "description": "Conmuta DNS al target tras health OK", "default": false },
                    "skip_provision": { "type": "boolean", "description": "Omite provisionar stack nuevo", "default": false }
                }
            }),
        ),
    ]
}

/* Gestion ops: instalar Coolify, websocket, scripts y comparacion BD. */
fn definiciones_gestion_ops() -> Vec<Value> {
    vec![
        tool_def(
            "coolify_install_coolify",
            "Instala Coolify en un target remoto via SSH",
            serde_json::json!({
                "type": "object",
                "required": ["target"],
                "properties": {
                    "target": { "type": "string", "description": "Nombre del target definido en settings.json" }
                }
            }),
        ),
        tool_def(
            "coolify_deploy_websocket",
            "Agrega servicio WebSocket (Bun) a un stack Kamples existente",
            serde_json::json!({
                "type": "object",
                "required": ["site_name"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio Kamples" }
                }
            }),
        ),
        tool_def(
            "coolify_run_script",
            "Sube un script local al contenedor y lo ejecuta",
            serde_json::json!({
                "type": "object",
                "required": ["site_name", "file_path"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "file_path": { "type": "string", "description": "Ruta al script local" },
                    "interpreter": { "type": "string", "description": "Interprete (php, bash, python3)" },
                    "target": { "type": "string", "description": "Contenedor objetivo", "default": "wordpress" },
                    "args": { "type": "string", "description": "Argumentos adicionales" }
                }
            }),
        ),
        tool_def(
            "coolify_db_compare",
            "Compara la base de datos de un sitio contra un dump o contra otro sitio (E12). Descubre todas las tablas automáticamente, soporta pgvector y devuelve reporte JSON estable.",
            serde_json::json!({
                "type": "object",
                "required": ["site_name"],
                "properties": {
                    "site_name": { "type": "string", "description": "Nombre del sitio" },
                    "dump": { "type": "string", "description": "Ruta al dump (local o VPS). Si se omite usa el último dump VPS" },
                    "against": { "type": "string", "description": "Nombre de otro sitio para comparar en vivo" },
                    "tables": { "type": "string", "description": "Limitar a tablas (separadas por coma)" },
                    "ignore_columns": { "type": "string", "description": "Columnas a ignorar (separadas por coma)" },
                    "limit_diff": { "type": "integer", "description": "Max filas de muestra por tabla", "default": 20 },
                    "no_tmp_container": { "type": "boolean", "description": "Modo ligero: solo conteos + hashes", "default": false },
                    "extract_limit": { "type": "integer", "description": "Max filas a extraer por tabla" }
                }
            }),
        ),
    ]
}

fn tool_def(name: &str, description: &str, input_schema: Value) -> Value {
    serde_json::json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}
