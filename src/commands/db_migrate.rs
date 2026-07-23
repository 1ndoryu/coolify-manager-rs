/*
 * Comando: db-migrate
 * Aplica archivos de migración SQL (.up.sql) pendientes contra la BD del sitio.
 * Lee los archivos de migración locales, compara con _sqlx_migrations, y ejecuta
 * solo las que faltan.
 *
 * Con --dry-run, envuelve cada migración en BEGIN/ROLLBACK.
 * Con --file, aplica un archivo SQL específico en vez de detectar pendientes.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::pg_utils;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;

use base64::Engine as _;
use std::path::{Path, PathBuf};

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    migrations_dir: Option<&Path>,
    file: Option<&Path>,
    dry_run: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    let stack_uuid = site.stack_uuid.as_deref().unwrap();
    let target_config = settings.resolve_site_target(site)?;

    let mut ssh = SshClient::from_vps(&target_config.vps);
    ssh.connect().await?;

    let (pg_container, db_user, db_name, _) =
        pg_utils::get_pg_credentials(&ssh, stack_uuid).await?;

    /* Modo: archivo único */
    if let Some(sql_file) = file {
        return apply_single_file(
            &ssh,
            &pg_container,
            &db_user,
            &db_name,
            sql_file,
            dry_run,
        )
        .await;
    }

    /* Modo: migraciones pendientes automáticas */
    let migrations_path = migrations_dir.unwrap_or_else(|| Path::new("migrations"));

    if !migrations_path.exists() {
        return Err(CoolifyError::Validation(format!(
            "Directorio de migraciones no encontrado: {}",
            migrations_path.display()
        )));
    }

    /* 1. Obtener migraciones ya aplicadas */
    let applied_sql = "SELECT version::text FROM _sqlx_migrations ORDER BY version;";
    let applied_output = pg_utils::run_pg_query(&ssh, &pg_container, &db_user, &db_name, applied_sql).await?;
    let applied_versions: Vec<String> = applied_output
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    println!("[db-migrate] {} — {} migraciones ya aplicadas", site_name, applied_versions.len());

    /* 2. Listar archivos .up.sql locales */
    let mut pending_files: Vec<(String, PathBuf)> = Vec::new();
    let entries = std::fs::read_dir(migrations_path).map_err(|e| {
        CoolifyError::Validation(format!("Error leyendo directorio de migraciones: {}", e))
    })?;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if !name.ends_with(".up.sql") {
            continue;
        }

        let version = match name.split('_').next() {
            Some(v) if v.chars().all(|c| c.is_ascii_digit()) => v.to_string(),
            _ => continue,
        };

        if applied_versions.contains(&version) {
            continue;
        }

        let description = name
            .strip_prefix(&format!("{}_", version))
            .and_then(|s| s.strip_suffix(".up.sql"))
            .unwrap_or("unknown");

        println!("  ⏳ Pendiente: {} — {}", version, description);
        pending_files.push((version, path));
    }

    if pending_files.is_empty() {
        println!();
        println!("  ✅ No hay migraciones pendientes.");
        return Ok(());
    }

    pending_files.sort_by(|a, b| a.0.cmp(&b.0));

    println!();
    println!(
        "  📦 {} migraciones pendientes{}",
        pending_files.len(),
        if dry_run { " (dry-run)" } else { "" }
    );
    println!();

    /* 3. Aplicar cada migración */
    let mut applied_count = 0u32;
    let mut error_count = 0u32;

    for (version, path) in &pending_files {
        let description = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix(&format!("{}_", version)))
            .and_then(|s| s.strip_suffix(".up.sql"))
            .unwrap_or("unknown");

        match apply_single_file(&ssh, &pg_container, &db_user, &db_name, path, dry_run).await {
            Ok(()) => {
                applied_count += 1;
                if !dry_run {
                    let checksum = compute_checksum(path)?;
                    let register_sql = format!(
                        "INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES ({}, '{}', NOW(), true, decode('{}', 'hex'), 0) ON CONFLICT (version) DO NOTHING;",
                        version,
                        description.replace('\'', "''"),
                        checksum
                    );
                    let _ = pg_utils::run_pg_query(&ssh, &pg_container, &db_user, &db_name, &register_sql).await;
                }
            }
            Err(e) => {
                eprintln!("  ❌ {} — {}: {}", version, description, e);
                error_count += 1;
                break;
            }
        }
    }

    println!();
    if dry_run {
        println!(
            "[db-migrate] dry-run completado: {} migraciones simuladas, {} errores",
            applied_count, error_count
        );
    } else {
        println!(
            "[db-migrate] {} migraciones aplicadas, {} errores",
            applied_count, error_count
        );
    }

    if error_count > 0 {
        return Err(CoolifyError::Validation(format!(
            "{} migraciones fallaron",
            error_count
        )));
    }

    Ok(())
}

async fn apply_single_file(
    ssh: &SshClient,
    pg_container: &str,
    db_user: &str,
    db_name: &str,
    sql_file: &Path,
    dry_run: bool,
) -> std::result::Result<(), CoolifyError> {
    let name = sql_file.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");

    let sql_content = std::fs::read_to_string(sql_file).map_err(|e| {
        CoolifyError::Validation(format!("Error leyendo {}: {}", sql_file.display(), e))
    })?;

    let final_sql = if dry_run {
        format!("BEGIN;\n{}\nROLLBACK;", sql_content)
    } else {
        sql_content
    };

    let sql_b64 = base64::engine::general_purpose::STANDARD.encode(final_sql.as_bytes());
    let cmd = format!(
        "echo '{}' | base64 -d | docker exec -i {} psql -U {} -d {} 2>&1",
        sql_b64, pg_container, db_user, db_name
    );

    let result = ssh.execute(&cmd).await?;

    if result.success() {
        println!("  {} {}", if dry_run { "🔍" } else { "✅" }, name);
    } else {
        let stderr = result.stderr.trim();
        let stdout = result.stdout.trim();
        if stderr.contains("ERROR") || stdout.contains("ERROR") {
            return Err(CoolifyError::Validation(format!(
                "SQL error en {}: {}",
                name,
                if !stderr.is_empty() { stderr } else { stdout }
            )));
        }
        println!("  ⚠️  {} (con warnings)", name);
    }

    Ok(())
}

fn compute_checksum(path: &Path) -> std::result::Result<String, CoolifyError> {
    use sha2::Digest;
    let content = std::fs::read(path).map_err(|e| {
        CoolifyError::Validation(format!("Error leyendo {}: {}", path.display(), e))
    })?;
    let hash = sha2::Sha256::digest(&content);
    Ok(hex_encode(&hash))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
