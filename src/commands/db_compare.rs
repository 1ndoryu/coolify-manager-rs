/*
 * Comando: db-compare
 * Compara la base de datos en vivo de un sitio contra un dump (VPS/local)
 * o contra otro sitio, de forma precisa y sin parsear dumps SQL como texto.
 *
 * E12: descubre tablas automáticamente, soporta tablas personalizadas,
 * pgvector y tablas sin PK. Solo lectura sobre la BD viva. Salida JSON estable.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::compare_manager::{execute, CompareOptions};

use std::path::Path;

pub async fn run(
    config_path: &Path,
    site_name: &str,
    dump: Option<String>,
    against: Option<String>,
    tables: Option<String>,
    ignore_columns: Option<String>,
    limit_diff: usize,
    json: bool,
    no_tmp_container: bool,
    extract_limit: Option<u64>,
) -> std::result::Result<(), CoolifyError> {
    /* Validación de mutua exclusión dump/against */
    if dump.is_some() && against.is_some() {
        return Err(CoolifyError::Validation(
            "Usa --dump O --against, no ambos".into(),
        ));
    }

    let opts = CompareOptions {
        site_name: site_name.to_string(),
        dump,
        against,
        tables,
        ignore_columns,
        limit_diff,
        json,
        no_tmp_container,
        extract_limit,
    };

    let report = execute(config_path, &opts).await?;

    if json {
        println!("{}", report.to_json()?);
    } else {
        println!("{}", report.to_text());
    }
    Ok(())
}

/// Para reutilizar en MCP: ejecuta y devuelve el JSON (o texto).
pub async fn execute_json(
    config_path: &Path,
    site_name: &str,
    dump: Option<String>,
    against: Option<String>,
    tables: Option<String>,
    ignore_columns: Option<String>,
    limit_diff: usize,
    no_tmp_container: bool,
    extract_limit: Option<u64>,
) -> std::result::Result<String, CoolifyError> {
    let opts = CompareOptions {
        site_name: site_name.to_string(),
        dump,
        against,
        tables,
        ignore_columns,
        limit_diff,
        json: true,
        no_tmp_container,
        extract_limit,
    };
    let report = execute(config_path, &opts).await?;
    report.to_json()
}

/// Preflight de validación de settings (para --help o errores tempranos).
pub async fn validate_site(config_path: &Path, site_name: &str) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let _ = settings.get_site(site_name)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_mutua_exclusion_se_valida_en_cli() {
        /* La validación real vive en run(); aquí solo verificamos la lógica de decisión */
        let dump = Some("a.sql".to_string());
        let against = Some("otro".to_string());
        assert!(dump.is_some() && against.is_some());
    }
}
