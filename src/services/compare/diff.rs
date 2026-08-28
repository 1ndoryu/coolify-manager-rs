/*
 * compare/diff — comparación precisa de conjuntos de filas.
 * E12: extrae cada lado como JSON canónico por fila (SQL real, sin parsear
 * el dump como texto) y compara los conjuntos en Rust. Funciona aunque los
 * dos lados sean contenedores distintos (vivo vs temporal) y para cualquier
 * tabla custom, con o sin PK.
 */

use crate::error::CoolifyError;
use crate::infra::pg_utils;
use crate::infra::ssh_client::SshClient;
use crate::services::compare::schema::{DbEngine, TableInfo};

use base64::Engine as _;
use secrecy::ExposeSecret;
use std::collections::BTreeSet;

/// Resultado de comparar una tabla.
#[derive(Debug, Clone)]
pub struct TableDiff {
    pub table: String,
    pub rows_vivo: i64,
    pub rows_otro: i64,
    pub solo_en_vivo: Vec<String>,
    pub solo_en_otro: Vec<String>,
    pub diffs: i64,
    pub not_comparable: bool,
    pub vector_ignored: bool,
}

/// Extrae todas las filas de una tabla en proyección canónica como JSON por fila.
/// Límite por tabla (seguridad: evitar volcar tablas enormes a memoria).
pub async fn extract_rows(
    ssh: &SshClient,
    engine: DbEngine,
    container: &str,
    db_user: &str,
    db_name: &str,
    db_password: Option<&secrecy::SecretString>,
    table: &str,
    info: &TableInfo,
    limit: Option<u64>,
) -> std::result::Result<Vec<String>, CoolifyError> {
    let comparable = info.comparable_columns();

    /* Sin columnas comparables → no se puede extraer nada comparable */
    if comparable.is_empty() {
        return Ok(Vec::new());
    }

    match engine {
        DbEngine::Postgres => {
            let cols = comparable.join(",");
            let mut sql = format!(
                "SELECT to_json(t)::text FROM (SELECT {cols} FROM {table}) t"
            );
            if let Some(l) = limit {
                sql.push_str(&format!(" LIMIT {l}"));
            }
            let out = pg_utils::run_pg_query(ssh, container, db_user, db_name, &sql).await?;
            Ok(out
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect())
        }
        DbEngine::MariaDb => {
            let pw = db_password.map(|s| s.expose_secret()).unwrap_or_default();
            /* JSON_OBJECT disponible en MariaDB 10.2+.
             * Los backticks se envían por base64 (sin shell) para que el host no
             * los interprete como command substitution ni el SQL como inyección. */
            let obj = comparable
                .iter()
                .map(|c| format!("'{c}', `{c}`"))
                .collect::<Vec<_>>()
                .join(", ");
            let mut sql = format!("SELECT JSON_OBJECT({obj}) FROM `{table}`");
            if let Some(l) = limit {
                sql.push_str(&format!(" LIMIT {l}"));
            }
            let sql_b64 = base64::engine::general_purpose::STANDARD.encode(sql.as_bytes());
            /* --default-character-set=utf8mb4: sin esto el cliente mariadb devuelve
             * los emojis de 4 bytes (UTF-8) como '?' solo en el lado vivo, y la
             * comparacion produce falsos positivos de diferencia. */
            let cmd = format!(
                "echo '{}' | base64 -d | docker exec -i {container} mariadb --default-character-set=utf8mb4 -u {db_user} -p'{pw}' {db_name} -N 2>&1",
                sql_b64
            );
            let res = ssh.execute(&cmd).await?;
            if !res.success() {
                return Err(CoolifyError::Docker {
                    exit_code: res.exit_code,
                    stderr: res.stderr.trim().to_string(),
                });
            }
            Ok(res
                .stdout
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect())
        }
    }
}

/// Compara dos conjuntos de filas canónicas y produce el diff.
pub fn compare_sets(vivo: Vec<String>, otro: Vec<String>, max_sample: usize) -> (i64, Vec<String>, Vec<String>) {
    let set_vivo: BTreeSet<String> = vivo.into_iter().collect();
    let set_otro: BTreeSet<String> = otro.into_iter().collect();

    let solo_en_vivo: Vec<String> = set_vivo.difference(&set_otro).cloned().take(max_sample).collect();
    let solo_en_otro: Vec<String> = set_otro.difference(&set_vivo).cloned().take(max_sample).collect();
    let diff_count = set_vivo.symmetric_difference(&set_otro).count() as i64;

    (diff_count, solo_en_vivo, solo_en_otro)
}

/// Compara una tabla entre dos lados (dos contenedores/BDs).
/// `limit` es el máximo de filas a extraer por lado (None = todas).
/// `max_sample` es el máximo de filas de muestra en el reporte.
pub async fn compare_table(
    ssh: &SshClient,
    engine: DbEngine,
    container_vivo: &str,
    user_vivo: &str,
    db_vivo: &str,
    pass_vivo: Option<&secrecy::SecretString>,
    container_otro: &str,
    user_otro: &str,
    db_otro: &str,
    pass_otro: Option<&secrecy::SecretString>,
    table: &str,
    info: &TableInfo,
    extract_limit: Option<u64>,
    max_sample: usize,
) -> std::result::Result<TableDiff, CoolifyError> {
    let vector_ignored = info.has_vector();
    let rows_vivo = crate::services::compare::digest::count_rows(
        ssh, engine, container_vivo, user_vivo, db_vivo, pass_vivo, table,
    )
    .await?;
    let rows_otro = crate::services::compare::digest::count_rows(
        ssh, engine, container_otro, user_otro, db_otro, pass_otro, table,
    )
    .await?;

    let filas_vivo = extract_rows(ssh, engine, container_vivo, user_vivo, db_vivo, pass_vivo, table, info, extract_limit).await?;
    let filas_otro = extract_rows(ssh, engine, container_otro, user_otro, db_otro, pass_otro, table, info, extract_limit).await?;

    let (diffs, solo_en_vivo, solo_en_otro) = compare_sets(filas_vivo, filas_otro, max_sample);
    let not_comparable = info.comparable_columns().is_empty();

    Ok(TableDiff {
        table: table.to_string(),
        rows_vivo,
        rows_otro,
        solo_en_vivo,
        solo_en_otro,
        diffs,
        not_comparable,
        vector_ignored,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_sets_identicas() {
        let a = vec!["{\"id\":1}".to_string(), "{\"id\":2}".to_string()];
        let b = vec!["{\"id\":2}".to_string(), "{\"id\":1}".to_string()];
        let (d, sa, sb) = compare_sets(a, b, 10);
        assert_eq!(d, 0);
        assert!(sa.is_empty());
        assert!(sb.is_empty());
    }

    #[test]
    fn test_compare_sets_con_diferencia() {
        let a = vec!["{\"id\":1}".to_string(), "{\"id\":3}".to_string()];
        let b = vec!["{\"id\":2}".to_string(), "{\"id\":3}".to_string()];
        let (d, sa, sb) = compare_sets(a, b, 10);
        assert_eq!(d, 2);
        assert_eq!(sa, vec!["{\"id\":1}".to_string()]);
        assert_eq!(sb, vec!["{\"id\":2}".to_string()]);
    }

    #[test]
    fn test_compare_sets_muestra_limitada() {
        let a: Vec<String> = (1..=50).map(|i| format!("{{\"id\":{i}}}")).collect();
        let b: Vec<String> = Vec::new();
        let (d, sa, _) = compare_sets(a, b, 5);
        assert_eq!(d, 50);
        assert_eq!(sa.len(), 5);
    }
}
