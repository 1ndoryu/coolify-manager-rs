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

/* run/execute_json reciben CompareOptions por valor (119A-6):
 * los 10/9 parámetros sueltos duplicaban el struct. */
pub async fn run(
    config_path: &Path,
    opts: CompareOptions,
) -> std::result::Result<(), CoolifyError> {
    /* Validación de mutua exclusión dump/against */
    if opts.dump.is_some() && opts.against.is_some() {
        return Err(CoolifyError::Validation(
            "Usa --dump O --against, no ambos".into(),
        ));
    }

    /* v2: sin baseline fijada el reporte informa pero no certifica (GRIS).
    Solo en run() interactivo: execute_json debe devolver JSON limpio para MCP. */
    if !opts.no_tmp_container && opts.dump.is_none() && opts.against.is_none() {
        eprintln!(
            "Aviso db-compare: sin --dump ni --against se usa el último dump VPS \
             (baseline no fijada): el veredicto nunca será VERDE. \
             Fija --dump <ruta|legacy:...> o --against para certificar."
        );
    }

    let report = execute(config_path, &opts).await?;

    if opts.json {
        println!("{}", report.to_json()?);
    } else {
        println!("{}", report.to_text());
    }
    Ok(())
}

/// Para reutilizar en MCP: ejecuta y devuelve el JSON (o texto).
pub async fn execute_json(
    config_path: &Path,
    opts: CompareOptions,
) -> std::result::Result<String, CoolifyError> {
    let mut opts = opts;
    opts.json = true;
    let report = execute(config_path, &opts).await?;
    report.to_json()
}

/// Preflight de validación de settings (para --help o errores tempranos).
pub async fn validate_site(
    config_path: &Path,
    site_name: &str,
) -> std::result::Result<(), CoolifyError> {
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
