/*
 * MCP Tools — herramientas disponibles via MCP.
 * Cada tool mapea a un comando CLI con el mismo handler.
 */

use crate::error::CoolifyError;

use serde_json::Value;
use std::path::{Path, PathBuf};

/// Retorna la definicion de todas las tools MCP.
pub fn list_tools() -> Vec<Value> {
    vec![
        tool_def("coolify_new_site", "Crea un nuevo sitio WordPress con tema Glory en Coolify", serde_json::json!({
            "type": "object",
            "required": ["site_name", "domain"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre unico del sitio (slug)" },
                "domain": { "type": "string", "description": "Dominio completo con protocolo (https://...)" },
                "glory_branch": { "type": "string", "description": "Rama del tema Glory", "default": "main" },
                "library_branch": { "type": "string", "description": "Rama de la libreria Glory", "default": "main" },
                "template": { "type": "string", "description": "Template de stack", "default": "wordpress", "enum": ["wordpress", "kamples", "minecraft"] },
                "target": { "type": "string", "description": "Target opcional definido en settings.json" },
                "skip_theme": { "type": "boolean", "description": "Omitir instalacion del tema", "default": false },
                "skip_cache": { "type": "boolean", "description": "Omitir cache headers", "default": false }
            }
        })),
        tool_def("coolify_deploy_theme", "Despliega o actualiza el tema Glory en un sitio existente", serde_json::json!({
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
        })),
        tool_def("coolify_list_sites", "Lista todos los sitios configurados con su estado", serde_json::json!({
            "type": "object",
            "properties": {
                "detailed": { "type": "boolean", "description": "Informacion detallada", "default": false }
            }
        })),
        tool_def("coolify_restart", "Reinicia los servicios de un sitio", serde_json::json!({
            "type": "object",
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" },
                "all": { "type": "boolean", "description": "Reiniciar todos", "default": false },
                "only_db": { "type": "boolean", "description": "Solo reiniciar contenedor de BD", "default": false },
                "only_wordpress": { "type": "boolean", "description": "Solo reiniciar contenedor WordPress", "default": false }
            }
        })),
        tool_def("coolify_import_db", "Importa un archivo SQL en la base de datos del sitio", serde_json::json!({
            "type": "object",
            "required": ["site_name", "sql_file_path"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" },
                "sql_file_path": { "type": "string", "description": "Ruta al archivo .sql" },
                "fix_urls": { "type": "boolean", "description": "Corregir URLs tras importar", "default": false }
            }
        })),
        tool_def("coolify_export_db", "Exporta la base de datos a un archivo SQL", serde_json::json!({
            "type": "object",
            "required": ["site_name"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" },
                "output_path": { "type": "string", "description": "Ruta de salida" }
            }
        })),
        tool_def("coolify_backup", "Crea o lista backups externos validados de un sitio", serde_json::json!({
            "type": "object",
            "required": ["site_name"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" },
                "tier": { "type": "string", "description": "Tier del backup", "default": "manual", "enum": ["daily", "weekly", "manual"] },
                "label": { "type": "string", "description": "Etiqueta opcional" },
                "list": { "type": "boolean", "description": "Lista backups existentes", "default": false }
            }
        })),
        tool_def("coolify_restore_backup", "Restaura un backup valido en un sitio", serde_json::json!({
            "type": "object",
            "required": ["site_name", "backup_id"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" },
                "backup_id": { "type": "string", "description": "Identificador del backup" },
                "skip_safety_snapshot": { "type": "boolean", "description": "Omitir snapshot previo", "default": false }
            }
        })),
        tool_def("coolify_health", "Ejecuta health checks del sitio", serde_json::json!({
            "type": "object",
            "required": ["site_name"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" }
            }
        })),
        tool_def("coolify_migrate", "Migra un sitio completo a otro target configurado", serde_json::json!({
            "type": "object",
            "required": ["site_name", "target"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" },
                "target": { "type": "string", "description": "Target definido en settings.json" },
                "dry_run": { "type": "boolean", "description": "Genera plan sin ejecutar", "default": false },
                "switch_dns": { "type": "boolean", "description": "Conmuta DNS al target tras health OK", "default": false }
            }
        })),
        tool_def("coolify_switch_dns", "Conmuta los registros DNS del sitio hacia una IP o target", serde_json::json!({
            "type": "object",
            "required": ["site_name"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" },
                "target": { "type": "string", "description": "Target definido en settings.json" },
                "target_ip": { "type": "string", "description": "IP explícita destino" },
                "dry_run": { "type": "boolean", "description": "Solo muestra acciones", "default": false }
            }
        })),
        tool_def("coolify_audit_vps", "Audita rendimiento y seguridad de una VPS o target", serde_json::json!({
            "type": "object",
            "properties": {
                "target": { "type": "string", "description": "Target opcional; si se omite usa la VPS principal" }
            }
        })),
        tool_def("coolify_wp_security", "Audita WordPress y permite rotar password admin", serde_json::json!({
            "type": "object",
            "required": ["site_name"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" },
                "audit": { "type": "boolean", "description": "Ejecutar auditoría", "default": true },
                "user": { "type": "string", "description": "Usuario admin a rotar" },
                "password": { "type": "string", "description": "Nueva password; si se omite se genera" }
            }
        })),
        tool_def("coolify_exec", "Ejecuta un comando dentro del contenedor del sitio", serde_json::json!({
            "type": "object",
            "required": ["site_name"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" },
                "command": { "type": "string", "description": "Comando bash" },
                "php_code": { "type": "string", "description": "Codigo PHP" },
                "target": { "type": "string", "description": "Contenedor objetivo", "default": "wordpress", "enum": ["wordpress", "mariadb"] }
            }
        })),
        tool_def("coolify_view_logs", "Obtiene logs del contenedor o debug.log", serde_json::json!({
            "type": "object",
            "required": ["site_name"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" },
                "lines": { "type": "integer", "description": "Lineas a mostrar", "default": 50 },
                "target": { "type": "string", "description": "Contenedor", "default": "wordpress" },
                "wp_debug": { "type": "boolean", "description": "Ver debug.log", "default": false },
                "filter": { "type": "string", "description": "Filtrar por patron" }
            }
        })),
        tool_def("coolify_debug", "Gestiona WP_DEBUG en un sitio", serde_json::json!({
            "type": "object",
            "required": ["site_name"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" },
                "enable": { "type": "boolean", "description": "Habilitar WP_DEBUG" },
                "disable": { "type": "boolean", "description": "Deshabilitar WP_DEBUG" }
            }
        })),
        tool_def("coolify_cache", "Gestiona cache headers HTTP del sitio", serde_json::json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" },
                "action": { "type": "string", "description": "Accion", "enum": ["status", "enable", "disable"] },
                "all": { "type": "boolean", "description": "Aplicar a todos", "default": false }
            }
        })),
        tool_def("coolify_git_status", "Estado de Git en el tema Glory remoto", serde_json::json!({
            "type": "object",
            "required": ["site_name"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" }
            }
        })),
        tool_def("coolify_set_domain", "Cambia el dominio de un sitio WordPress", serde_json::json!({
            "type": "object",
            "required": ["site_name", "new_domain"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" },
                "new_domain": { "type": "string", "description": "Nuevo dominio con https://" }
            }
        })),
        tool_def("coolify_redeploy", "Fuerza un redeploy del servicio", serde_json::json!({
            "type": "object",
            "required": ["site_name"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" }
            }
        })),
        tool_def("coolify_setup_smtp", "Configura SMTP relay en el sitio", serde_json::json!({
            "type": "object",
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" },
                "all": { "type": "boolean", "description": "Todos los sitios", "default": false },
                "test": { "type": "boolean", "description": "Enviar email de prueba", "default": false },
                "test_email": { "type": "string", "description": "Email destino para prueba" }
            }
        })),
        tool_def("coolify_minecraft", "Gestiona servidores Minecraft", serde_json::json!({
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
        })),
        tool_def("coolify_failover", "Failover: restaura un sitio en VPS alternativo usando backup de Drive (no requiere VPS origen)", serde_json::json!({
            "type": "object",
            "required": ["site_name", "target"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" },
                "target": { "type": "string", "description": "Target destino definido en settings.json" },
                "backup_id": { "type": "string", "description": "ID de backup especifico; si se omite usa el mas reciente" },
                "switch_dns": { "type": "boolean", "description": "Conmuta DNS al target tras health OK", "default": false },
                "skip_provision": { "type": "boolean", "description": "Omite provisionar stack nuevo", "default": false }
            }
        })),
        tool_def("coolify_install_coolify", "Instala Coolify en un target remoto via SSH", serde_json::json!({
            "type": "object",
            "required": ["target"],
            "properties": {
                "target": { "type": "string", "description": "Nombre del target definido en settings.json" }
            }
        })),
        tool_def("coolify_deploy_websocket", "Agrega servicio WebSocket (Bun) a un stack Kamples existente", serde_json::json!({
            "type": "object",
            "required": ["site_name"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio Kamples" }
            }
        })),
        tool_def("coolify_run_script", "Sube un script local al contenedor y lo ejecuta", serde_json::json!({
            "type": "object",
            "required": ["site_name", "file_path"],
            "properties": {
                "site_name": { "type": "string", "description": "Nombre del sitio" },
                "file_path": { "type": "string", "description": "Ruta al script local" },
                "interpreter": { "type": "string", "description": "Interprete (php, bash, python3). Auto-detecta si se omite" },
                "target": { "type": "string", "description": "Contenedor objetivo", "default": "wordpress" },
                "args": { "type": "string", "description": "Argumentos adicionales para el script" }
            }
        })),
    ]
}

fn tool_def(name: &str, description: &str, input_schema: Value) -> Value {
    serde_json::json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

/// Ejecuta una tool por nombre y retorna el resultado como texto.
pub async fn call_tool(config_path: &Path, name: &str, args: Value) -> std::result::Result<String, CoolifyError> {
    let config_path = config_path.to_path_buf();

    match name {
        "coolify_new_site" => {
            let site_name = get_str(&args, "site_name")?;
            let domain = get_str(&args, "domain")?;
            let glory_branch = get_str_or(&args, "glory_branch", "main");
            let library_branch = get_str_or(&args, "library_branch", "main");
            let template = get_str_or(&args, "template", "wordpress");
            let target = args.get("target").and_then(|v| v.as_str()).map(|s| s.to_string());
            let skip_theme = get_bool(&args, "skip_theme");
            let skip_cache = get_bool(&args, "skip_cache");

            crate::commands::new_site::execute(
                &config_path, &site_name, &domain, &glory_branch,
                &library_branch, &template, target.as_deref(), skip_theme, skip_cache,
            ).await?;
            Ok(format!("Sitio '{site_name}' creado exitosamente en {domain}"))
        }

        "coolify_deploy_theme" => {
            let site_name = get_str(&args, "site_name")?;
            let glory_branch = args.get("glory_branch").and_then(|v| v.as_str()).map(|s| s.to_string());
            let library_branch = args.get("library_branch").and_then(|v| v.as_str()).map(|s| s.to_string());
            let update = get_bool(&args, "update");
            let skip_react = get_bool(&args, "skip_react");
            let force = get_bool(&args, "force");

            crate::commands::deploy_theme::execute(
                &config_path, &site_name, glory_branch.as_deref(),
                library_branch.as_deref(), update, skip_react, force,
            ).await?;
            Ok(format!("Tema desplegado en '{site_name}'"))
        }

        "coolify_list_sites" => {
            let detailed = get_bool(&args, "detailed");
            crate::commands::list_sites::execute(&config_path, detailed).await?;
            Ok("Lista de sitios mostrada".to_string())
        }

        "coolify_restart" => {
            let site_name = args.get("site_name").and_then(|v| v.as_str()).map(|s| s.to_string());
            let all = get_bool(&args, "all");
            let only_db = get_bool(&args, "only_db");
            let only_wordpress = get_bool(&args, "only_wordpress");

            crate::commands::restart_site::execute(
                &config_path, site_name.as_deref(), all, only_db, only_wordpress,
            ).await?;
            Ok("Servicio(s) reiniciado(s)".to_string())
        }

        "coolify_import_db" => {
            let site_name = get_str(&args, "site_name")?;
            let sql_file = get_str(&args, "sql_file_path")?;
            let fix_urls = get_bool(&args, "fix_urls");

            crate::commands::import_database::execute(
                &config_path, &site_name, &PathBuf::from(&sql_file), fix_urls,
            ).await?;
            Ok(format!("Base de datos importada en '{site_name}'"))
        }

        "coolify_export_db" => {
            let site_name = get_str(&args, "site_name")?;
            let output = args.get("output_path").and_then(|v| v.as_str()).map(PathBuf::from);

            crate::commands::export_database::execute(
                &config_path, &site_name, output.as_deref(),
            ).await?;
            Ok(format!("Base de datos exportada de '{site_name}'"))
        }

        "coolify_backup" => {
            let site_name = get_str(&args, "site_name")?;
            let tier = get_str_or(&args, "tier", "manual");
            let label = args.get("label").and_then(|v| v.as_str()).map(|s| s.to_string());
            let list = get_bool(&args, "list");
            crate::commands::backup_site::execute(&config_path, &site_name, &tier, label.as_deref(), list).await?;
            Ok(if list {
                format!("Backups listados para '{site_name}'")
            } else {
                format!("Backup creado para '{site_name}'")
            })
        }

        "coolify_restore_backup" => {
            let site_name = get_str(&args, "site_name")?;
            let backup_id = get_str(&args, "backup_id")?;
            let skip_safety_snapshot = get_bool(&args, "skip_safety_snapshot");
            crate::commands::restore_backup::execute(&config_path, &site_name, &backup_id, skip_safety_snapshot).await?;
            Ok(format!("Backup '{backup_id}' restaurado en '{site_name}'"))
        }

        "coolify_health" => {
            let site_name = get_str(&args, "site_name")?;
            crate::commands::health_check::execute(&config_path, &site_name).await?;
            Ok(format!("Health check ejecutado para '{site_name}'"))
        }

        "coolify_migrate" => {
            let site_name = get_str(&args, "site_name")?;
            let target = get_str(&args, "target")?;
            let dry_run = get_bool(&args, "dry_run");
            let switch_dns = get_bool(&args, "switch_dns");
            crate::commands::migrate_site::execute(&config_path, &site_name, &target, dry_run, switch_dns).await?;
            Ok(format!("Migracion ejecutada para '{site_name}' hacia '{target}'"))
        }

        "coolify_switch_dns" => {
            let site_name = get_str(&args, "site_name")?;
            let target = args.get("target").and_then(|v| v.as_str()).map(|s| s.to_string());
            let target_ip = args.get("target_ip").and_then(|v| v.as_str()).map(|s| s.to_string());
            let dry_run = get_bool(&args, "dry_run");
            crate::commands::switch_dns::execute(&config_path, &site_name, target.as_deref(), target_ip.as_deref(), dry_run).await?;
            Ok(format!("DNS conmutado para '{site_name}'"))
        }

        "coolify_audit_vps" => {
            let target = args.get("target").and_then(|v| v.as_str()).map(|s| s.to_string());
            crate::commands::audit_vps::execute(&config_path, target.as_deref()).await?;
            Ok("Auditoria VPS completada".to_string())
        }

        "coolify_wp_security" => {
            let site_name = get_str(&args, "site_name")?;
            let audit = args.get("audit").and_then(|v| v.as_bool()).unwrap_or(true);
            let user = args.get("user").and_then(|v| v.as_str()).map(|s| s.to_string());
            let password = args.get("password").and_then(|v| v.as_str()).map(|s| s.to_string());
            crate::commands::wordpress_security::execute(&config_path, &site_name, audit, user.as_deref(), password.as_deref()).await?;
            Ok(format!("Auditoria WordPress completada para '{site_name}'"))
        }

        "coolify_exec" => {
            let site_name = get_str(&args, "site_name")?;
            let command = args.get("command").and_then(|v| v.as_str()).map(|s| s.to_string());
            let php_code = args.get("php_code").and_then(|v| v.as_str()).map(|s| s.to_string());
            let target = get_str_or(&args, "target", "wordpress");

            crate::commands::exec_command::execute(
                &config_path, &site_name, command.as_deref(), php_code.as_deref(), &target,
            ).await?;
            Ok("Comando ejecutado".to_string())
        }

        "coolify_view_logs" => {
            let site_name = get_str(&args, "site_name")?;
            let lines = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(50) as u32;
            let target = get_str_or(&args, "target", "wordpress");
            let wp_debug = get_bool(&args, "wp_debug");
            let filter = args.get("filter").and_then(|v| v.as_str()).map(|s| s.to_string());

            crate::commands::view_logs::execute(
                &config_path, &site_name, lines, &target, wp_debug, filter.as_deref(),
            ).await?;
            Ok("Logs obtenidos".to_string())
        }

        "coolify_debug" => {
            let site_name = get_str(&args, "site_name")?;
            let enable = get_bool(&args, "enable");
            let disable = get_bool(&args, "disable");

            crate::commands::debug_site::execute(
                &config_path, &site_name, enable, disable, !enable && !disable,
            ).await?;
            Ok("WP_DEBUG actualizado".to_string())
        }

        "coolify_cache" => {
            let site_name = args.get("site_name").and_then(|v| v.as_str()).map(|s| s.to_string());
            let action = get_str(&args, "action")?;
            let all = get_bool(&args, "all");

            crate::commands::cache_site::execute(
                &config_path, site_name.as_deref(), &action, all,
            ).await?;
            Ok("Cache actualizado".to_string())
        }

        "coolify_git_status" => {
            let site_name = get_str(&args, "site_name")?;
            crate::commands::git_status::execute(&config_path, &site_name).await?;
            Ok("Estado de Git obtenido".to_string())
        }

        "coolify_set_domain" => {
            let site_name = get_str(&args, "site_name")?;
            let new_domain = get_str(&args, "new_domain")?;
            crate::commands::set_domain::execute(&config_path, &site_name, &new_domain).await?;
            Ok(format!("Dominio actualizado a '{new_domain}'"))
        }

        "coolify_redeploy" => {
            let site_name = get_str(&args, "site_name")?;
            crate::commands::redeploy::execute(&config_path, &site_name).await?;
            Ok(format!("Redeploy iniciado para '{site_name}'"))
        }

        "coolify_setup_smtp" => {
            let site_name = args.get("site_name").and_then(|v| v.as_str()).map(|s| s.to_string());
            let all = get_bool(&args, "all");
            let test = get_bool(&args, "test");
            let test_email = args.get("test_email").and_then(|v| v.as_str()).map(|s| s.to_string());

            crate::commands::setup_smtp::execute(
                &config_path, site_name.as_deref(), all, test, test_email.as_deref(), false,
            ).await?;
            Ok("SMTP configurado".to_string())
        }

        "coolify_minecraft" => {
            let action = get_str(&args, "action")?;
            let server_name = get_str(&args, "server_name")?;
            let memory = get_str_or(&args, "memory", "2G");
            let max_players = args.get("max_players").and_then(|v| v.as_u64()).unwrap_or(20) as u32;
            let difficulty = get_str_or(&args, "difficulty", "normal");
            let console_cmd = args.get("console_command").and_then(|v| v.as_str()).map(|s| s.to_string());
            let lines = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(100) as u32;

            crate::commands::minecraft::execute(
                &config_path, &action, &server_name, &memory, max_players,
                &difficulty, "LATEST", 25565, console_cmd.as_deref(), lines,
            ).await?;
            Ok(format!("Minecraft '{server_name}': {action}"))
        }

        "coolify_failover" => {
            let site_name = get_str(&args, "site_name")?;
            let target = get_str(&args, "target")?;
            let backup_id = args.get("backup_id").and_then(|v| v.as_str()).map(|s| s.to_string());
            let switch_dns = get_bool(&args, "switch_dns");
            let skip_provision = get_bool(&args, "skip_provision");
            crate::commands::failover::execute(
                &config_path, &site_name, &target, backup_id.as_deref(), switch_dns, skip_provision,
            ).await?;
            Ok(format!("Failover completado: '{site_name}' -> '{target}'"))
        }

        "coolify_install_coolify" => {
            let target = get_str(&args, "target")?;
            crate::commands::install_coolify::execute(&config_path, &target).await?;
            Ok(format!("Coolify instalado en target '{target}'"))
        }

        "coolify_deploy_websocket" => {
            let site_name = get_str(&args, "site_name")?;
            crate::commands::deploy_websocket::execute(&config_path, &site_name).await?;
            Ok(format!("WebSocket desplegado en '{site_name}'"))
        }

        "coolify_run_script" => {
            let site_name = get_str(&args, "site_name")?;
            let file_path = get_str(&args, "file_path")?;
            let interpreter = args.get("interpreter").and_then(|v| v.as_str()).map(|s| s.to_string());
            let target = get_str_or(&args, "target", "wordpress");
            let script_args = args.get("args").and_then(|v| v.as_str()).map(|s| s.to_string());
            crate::commands::run_script::execute(
                &config_path, &site_name, &PathBuf::from(&file_path),
                interpreter.as_deref(), &target, script_args.as_deref(),
            ).await?;
            Ok(format!("Script ejecutado en '{site_name}'"))
        }

        _ => Err(CoolifyError::Validation(format!("Tool '{name}' no existe"))),
    }
}

/* Helpers para extraer valores de args JSON */

fn get_str(args: &Value, key: &str) -> std::result::Result<String, CoolifyError> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| CoolifyError::Validation(format!("Parametro requerido: '{key}'")))
}

fn get_str_or(args: &Value, key: &str, default: &str) -> String {
    args.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(default)
        .to_string()
}

fn get_bool(args: &Value, key: &str) -> bool {
    args.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}
