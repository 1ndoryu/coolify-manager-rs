/*
 * compare/schema — descubrimiento de esquema (PostgreSQL y MariaDB).
 * E12: la comparación NUNCA hardcodea tablas: se descubre todo lo que existe
 * en cada lado (information_schema / SHOW TABLES) y se comparan los conjuntos.
 */

use crate::error::CoolifyError;
use crate::infra::pg_utils;
use crate::infra::ssh_client::SshClient;
use crate::services::database_manager;

use secrecy::ExposeSecret;

/// Motor de base de datos soportado por db-compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbEngine {
    Postgres,
    MariaDb,
}

impl DbEngine {
    pub fn as_str(&self) -> &'static str {
        match self {
            DbEngine::Postgres => "postgres",
            DbEngine::MariaDb => "mariadb",
        }
    }
}

/// Una columna con su tipo (para detectar vector/bytea y proyecciones).
#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub is_vector: bool,
    pub is_bytea: bool,
}

/// Una tabla con su esquema y capacidades de comparación.
#[derive(Debug, Clone, Default)]
pub struct TableInfo {
    pub columns: Vec<ColumnInfo>,
    pub has_pk: bool,
}

impl TableInfo {
    /// Columnas no-especiales (sin vector ni bytea) usadas para la proyección canónica.
    pub fn comparable_columns(&self) -> Vec<&str> {
        self.columns
            .iter()
            .filter(|c| !c.is_vector && !c.is_bytea)
            .map(|c| c.name.as_str())
            .collect()
    }

    pub fn has_vector(&self) -> bool {
        self.columns.iter().any(|c| c.is_vector)
    }

    pub fn has_bytea(&self) -> bool {
        self.columns.iter().any(|c| c.is_bytea)
    }
}

/// Esquema completo de una base de datos (ambos lados usan esta estructura).
#[derive(Debug, Clone)]
pub struct SchemaModel {
    pub engine: DbEngine,
    pub tables: std::collections::BTreeMap<String, TableInfo>,
}

impl Default for SchemaModel {
    fn default() -> Self {
        Self {
            engine: DbEngine::Postgres,
            tables: std::collections::BTreeMap::new(),
        }
    }
}

/// Descubre el esquema completo de un contenedor PostgreSQL.
pub async fn discover_postgres(
    ssh: &SshClient,
    pg_container: &str,
    db_user: &str,
    db_name: &str,
) -> std::result::Result<SchemaModel, CoolifyError> {
    /* Tablas del schema public (evita tablas del sistema y extensiones) */
    let tables_sql = "SELECT table_name FROM information_schema.tables \
                      WHERE table_schema='public' AND table_type='BASE TABLE' ORDER BY table_name";
    let out = pg_utils::run_pg_query(ssh, pg_container, db_user, db_name, tables_sql).await?;
    let tables: Vec<String> = out
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    let mut model = SchemaModel {
        engine: DbEngine::Postgres,
        tables: std::collections::BTreeMap::new(),
    };

    for t in &tables {
        pg_utils::validate_table_name(t)?;
        let info = discover_postgres_table(ssh, pg_container, db_user, db_name, t).await?;
        model.tables.insert(t.clone(), info);
    }
    Ok(model)
}

async fn discover_postgres_table(
    ssh: &SshClient,
    pg_container: &str,
    db_user: &str,
    db_name: &str,
    table: &str,
) -> std::result::Result<TableInfo, CoolifyError> {
    let cols_sql = format!(
        "SELECT column_name, data_type FROM information_schema.columns \
         WHERE table_schema='public' AND table_name='{}' ORDER BY ordinal_position",
        table
    );
    let out = pg_utils::run_pg_query(ssh, pg_container, db_user, db_name, &cols_sql).await?;

    let mut columns = Vec::new();
    for line in out.lines() {
        let mut parts = line.splitn(2, '|');
        let name = parts.next().unwrap_or("").trim().to_string();
        let data_type = parts.next().unwrap_or("").trim().to_string();
        if name.is_empty() {
            continue;
        }
        let is_vector = data_type == "vector";
        let is_bytea = data_type == "bytea";
        columns.push(ColumnInfo {
            name,
            data_type,
            is_vector,
            is_bytea,
        });
    }

    /* Detectar PK via pg_constraint (tabla sin PK se compara igual con EXCEPT) */
    let pk_sql = format!(
        "SELECT 1 FROM pg_class c JOIN pg_constraint con ON con.conrelid=c.oid \
         WHERE c.relname='{}' AND con.contype='p' LIMIT 1",
        table
    );
    let pk_out = pg_utils::run_pg_query(ssh, pg_container, db_user, db_name, &pk_sql).await?;
    let has_pk = !pk_out.trim().is_empty();

    Ok(TableInfo { columns, has_pk })
}

/// Descubre el esquema completo de un contenedor MariaDB (WordPress).
pub async fn discover_mariadb(
    ssh: &SshClient,
    mariadb_container: &str,
    db_name: &str,
    db_user: &str,
    db_password: &secrecy::SecretString,
) -> std::result::Result<SchemaModel, CoolifyError> {
    let pw = db_password.expose_secret();
    let base = format!(
        "docker exec -i {mariadb_container} mariadb -u {db_user} -p'{pw}' {db_name} -N -e"
    );

    /* SHOW TABLES */
    let tables_cmd = format!("{base} \"SHOW TABLES;\"");
    let res = ssh.execute(&tables_cmd).await?;
    if !res.success() {
        return Err(CoolifyError::Docker {
            exit_code: res.exit_code,
            stderr: res.stderr.trim().to_string(),
        });
    }
    let tables: Vec<String> = res
        .stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    let mut model = SchemaModel {
        engine: DbEngine::MariaDb,
        tables: std::collections::BTreeMap::new(),
    };

    for t in &tables {
        /* MariaDB permite más caracteres en nombres; validamos solo lo estrictamente seguro */
        if !t
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            tracing::warn!("Tabla MariaDB con nombre no seguro omitida: '{}'", t);
            continue;
        }
        let info = discover_mariadb_table(ssh, mariadb_container, db_name, db_user, pw, t).await?;
        model.tables.insert(t.clone(), info);
    }
    Ok(model)
}

async fn discover_mariadb_table(
    ssh: &SshClient,
    mariadb_container: &str,
    db_name: &str,
    db_user: &str,
    db_password: &str,
    table: &str,
) -> std::result::Result<TableInfo, CoolifyError> {
    let base = format!(
        "docker exec -i {mariadb_container} mariadb -u {db_user} -p'{db_password}' {db_name} -N -e"
    );

    /* SHOW COLUMNS — columna Field, Type */
    let cols_cmd = format!("{base} \"SHOW COLUMNS FROM {table};\"");
    let res = ssh.execute(&cols_cmd).await?;
    if !res.success() {
        return Err(CoolifyError::Docker {
            exit_code: res.exit_code,
            stderr: res.stderr.trim().to_string(),
        });
    }

    let mut columns = Vec::new();
    for line in res.stdout.lines() {
        let mut parts = line.splitn(2, '\t');
        let name = parts.next().unwrap_or("").trim().to_string();
        let data_type = parts.next().unwrap_or("").trim().to_string();
        if name.is_empty() {
            continue;
        }
        let lower = data_type.to_lowercase();
        let is_vector = lower.starts_with("vector");
        let is_bytea = lower.contains("blob") || lower.contains("binary");
        columns.push(ColumnInfo {
            name,
            data_type,
            is_vector,
            is_bytea,
        });
    }

    /* Detectar PK */
    let pk_cmd = format!(
        "{base} \"SHOW INDEX FROM {table} WHERE Key_name='PRIMARY';\""
    );
    let pk_res = ssh.execute(&pk_cmd).await?;
    let has_pk = pk_res.success() && !pk_res.stdout.trim().is_empty();

    Ok(TableInfo { columns, has_pk })
}

/// Obtiene credenciales MariaDB (WordPress) — reutiliza database_manager.
pub async fn resolve_mariadb_credentials(
    ssh: &SshClient,
    wp_container: &str,
) -> std::result::Result<(String, String, secrecy::SecretString), CoolifyError> {
    database_manager::resolve_wordpress_credentials(ssh, wp_container).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comparable_columns_excluye_vector_bytea() {
        let info = TableInfo {
            columns: vec![
                ColumnInfo { name: "id".into(), data_type: "integer".into(), is_vector: false, is_bytea: false },
                ColumnInfo { name: "embedding".into(), data_type: "vector".into(), is_vector: true, is_bytea: false },
                ColumnInfo { name: "blob".into(), data_type: "bytea".into(), is_vector: false, is_bytea: true },
            ],
            has_pk: true,
        };
        let cols = info.comparable_columns();
        assert_eq!(cols, vec!["id"]);
        assert!(info.has_vector());
        assert!(info.has_bytea());
    }

    #[test]
    fn test_engine_as_str() {
        assert_eq!(DbEngine::Postgres.as_str(), "postgres");
        assert_eq!(DbEngine::MariaDb.as_str(), "mariadb");
    }
}
