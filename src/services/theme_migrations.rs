/*
 * ThemeMigrations — runner de migraciones SQL del tema Glory.
 * Separado de theme_manager (instalación/actualización del tema) para acotar
 * el ciclo de migraciones: tracking idempotente en PG + ejecución versionada.
 */

use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;

/// Ejecuta las migraciones SQL pendientes contra el contenedor PostgreSQL del stack.
///
/// Estrategia:
/// - Lee los archivos .sql del directorio de migraciones del tema (orden alfabetico garantiza version correcta).
/// - Usa una tabla `_migraciones_ejecutadas` en PG para tracking idempotente.
/// - Solo ejecuta archivos cuyo nombre todavia no esta registrado.
/// - Los archivos v001_* (schema inicial/base) se saltan — asume que BD ya existe.
pub(crate) async fn run_pending_migrations(
    ssh: &SshClient,
    wp_container_id: &str,
    pg_container_id: &str,
    theme_name: &str,
    pg_user: &str,
    pg_db: &str,
) -> std::result::Result<(), CoolifyError> {
    let migrations_dir =
        format!("/var/www/html/wp-content/themes/{theme_name}/App/Kamples/Database/migrations");

    /* Asegurar tabla de tracking en PG */
    let create_tracking = format!(
        "psql -U {pg_user} -d {pg_db} -c \"CREATE TABLE IF NOT EXISTS _migraciones_ejecutadas (nombre VARCHAR(255) PRIMARY KEY, ejecutada_en TIMESTAMPTZ DEFAULT NOW());\"",
    );
    let result = docker::docker_exec(ssh, pg_container_id, &create_tracking).await?;
    if !result.success() {
        tracing::warn!(
            "No se pudo crear tabla de tracking de migraciones: {}",
            result.stderr
        );
        return Ok(());
    }

    /* Listar archivos SQL disponibles ordenados */
    let list_cmd = format!("ls {migrations_dir}/v*.sql 2>/dev/null | sort");
    let list_result = docker::docker_exec(ssh, wp_container_id, &list_cmd).await?;
    if list_result.stdout.trim().is_empty() {
        tracing::info!("No se encontraron archivos .sql en {migrations_dir}");
        return Ok(());
    }

    let sql_files: Vec<&str> = list_result.stdout.trim().lines().collect();
    tracing::info!("Migraciones disponibles: {}", sql_files.len());

    for file_path in sql_files {
        let file_name = file_path.split('/').next_back().unwrap_or(file_path);

        /* Saltar schemas iniciales — la BD ya fue creada en el deploy inicial */
        if file_name.starts_with("v001_") {
            tracing::debug!("Saltando schema inicial: {file_name}");
            continue;
        }

        /* Verificar si ya fue ejecutada */
        let check_cmd = format!(
            "psql -U {pg_user} -d {pg_db} -t -c \"SELECT COUNT(*) FROM _migraciones_ejecutadas WHERE nombre = '{file_name}';\"",
        );
        let check = docker::docker_exec(ssh, pg_container_id, &check_cmd).await?;
        let count: i32 = check.stdout.trim().parse().unwrap_or(0);
        if count > 0 {
            tracing::debug!("Migracion ya ejecutada: {file_name}");
            continue;
        }

        /* Copiar SQL del contenedor WP al contenedor PG via docker cp en el host */
        tracing::info!("Ejecutando migracion: {file_name}");

        /* Extraer SQL del contenedor WordPress y ejecutarlo en PG */
        let exec_cmd = format!(
            "SQL=$(docker exec {wp_container_id} cat {file_path}); echo \"$SQL\" | docker exec -i {pg_container_id} psql -U {pg_user} -d {pg_db} 2>&1",
        );
        let exec_result = ssh.execute(&exec_cmd).await?;

        if exec_result.success() {
            /* Registrar como ejecutada */
            let register_cmd = format!(
                "psql -U {pg_user} -d {pg_db} -c \"INSERT INTO _migraciones_ejecutadas (nombre) VALUES ('{file_name}') ON CONFLICT DO NOTHING;\"",
            );
            let _ = docker::docker_exec(ssh, pg_container_id, &register_cmd).await;
            tracing::info!("Migracion completada: {file_name}");
        } else {
            /* Errores de columna ya existente o constraint duplicado son no-fatales (IF NOT EXISTS) */
            let stderr = &exec_result.stderr;
            if stderr.contains("already exists") || stderr.contains("duplicate") {
                tracing::warn!(
                    "Migracion {file_name} con warnings no fatales (ya existe): {stderr}"
                );
                let register_cmd = format!(
                    "psql -U {pg_user} -d {pg_db} -c \"INSERT INTO _migraciones_ejecutadas (nombre) VALUES ('{file_name}') ON CONFLICT DO NOTHING;\"",
                );
                let _ = docker::docker_exec(ssh, pg_container_id, &register_cmd).await;
            } else {
                tracing::error!("Error en migracion {file_name}: {stderr}");
            }
        }
    }

    tracing::info!("Runner de migraciones completado");
    Ok(())
}
