/* Split 119A-5 de compare_manager.rs — tipos de la comparación E12.
 * Re-exportados sin cambios desde compare_manager/mod.rs. */

use crate::services::compare::schema::DbEngine;

use secrecy::SecretString;

/// Opciones de la comparación.
#[derive(Debug, Clone)]
pub struct CompareOptions {
    pub site_name: String,
    /// Ruta al dump (local o VPS). Si None y `against` es None → último dump VPS.
    pub dump: Option<String>,
    /// Nombre de otro sitio configurado para comparar en vivo.
    pub against: Option<String>,
    /// Limitar a tablas concretas (comma-separated). None = todas.
    pub tables: Option<String>,
    /// Columnas volátiles a ignorar (comma-separated).
    pub ignore_columns: Option<String>,
    /// Máx filas de muestra por tabla.
    pub limit_diff: usize,
    /// Salida JSON (true) o texto (false).
    pub json: bool,
    /// Modo ligero (sin contenedor temporal) — solo conteos + hashes.
    pub no_tmp_container: bool,
    /// Máx filas a extraer por tabla (seguridad).
    pub extract_limit: Option<u64>,
}

/// Credenciales resueltas de un lado.
pub(super) struct SideCreds {
    pub(super) engine: DbEngine,
    pub(super) container: String,
    pub(super) db_user: String,
    pub(super) db_name: String,
    pub(super) db_password: Option<SecretString>,
}
