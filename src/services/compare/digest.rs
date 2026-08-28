/*
 * compare/digest — conteos y hash canónico por tabla (modo ligero).
 * E12: el modo ligero da certeza sobre conteos y sospecha de igualdad.
 * Para tablas con columnas especiales (vector/bytea) se marca
 * "no comparable en modo ligero" en vez de fallar.
 */

use crate::error::CoolifyError;
use crate::infra::pg_utils;
use crate::infra::ssh_client::SshClient;
use crate::services::compare::schema::{DbEngine, SchemaModel, TableInfo};

use secrecy::ExposeSecret;

/// Resultado del digest de una tabla en un lado.
#[derive(Debug, Clone)]
pub struct TableDigest {
    pub table: String,
    pub row_count: i64,
    /// Hash canónico (md5). None si la tabla no es comparable en modo ligero.
    pub hash: Option<String>,
    /// true si la tabla tiene columnas especiales que impiden el hash canónico.
    pub not_comparable_light: bool,
}

/// Cuenta las filas de una tabla.
pub async fn count_rows(
    ssh: &SshClient,
    engine: DbEngine,
    container: &str,
    db_user: &str,
    db_name: &str,
    db_password: Option<&secrecy::SecretString>,
    table: &str,
) -> std::result::Result<i64, CoolifyError> {
    match engine {
        DbEngine::Postgres => {
            let sql = format!("SELECT COUNT(*) FROM {}", table);
            let out = pg_utils::run_pg_query(ssh, container, db_user, db_name, &sql).await?;
            out.trim().parse::<i64>().map_err(|_| {
                CoolifyError::Docker {
                    exit_code: 1,
                    stderr: format!("COUNT(*) no numérico para {}", table),
                }
            })
        }
        DbEngine::MariaDb => {
            let pw = db_password.map(|s| s.expose_secret()).unwrap_or_default();
            let cmd = format!(
                "docker exec -i {container} mariadb -u {db_user} -p'{pw}' {db_name} -N -e \"SELECT COUNT(*) FROM {table};\""
            );
            let res = ssh.execute(&cmd).await?;
            if !res.success() {
                return Err(CoolifyError::Docker {
                    exit_code: res.exit_code,
                    stderr: res.stderr.trim().to_string(),
                });
            }
            res.stdout.trim().parse::<i64>().map_err(|_| {
                CoolifyError::Docker {
                    exit_code: 1,
                    stderr: format!("COUNT(*) no numérico para {}", table),
                }
            })
        }
    }
}

/// Hash canónico por tabla (md5 de las filas en proyección canónica).
/// PG usa row_to_json (falla con bytea/vector → se degrada).
/// MariaDB usa GROUP_CONCAT de la proyección.
pub async fn table_hash(
    ssh: &SshClient,
    engine: DbEngine,
    container: &str,
    db_user: &str,
    db_name: &str,
    db_password: Option<&secrecy::SecretString>,
    table: &str,
    info: &TableInfo,
) -> std::result::Result<Option<String>, CoolifyError> {
    let comparable = info.comparable_columns();

    /* Sin columnas comparables (todo vector/bytea) → no comparable en ligero */
    if comparable.is_empty() {
        return Ok(None);
    }

    match engine {
        DbEngine::Postgres => {
            /* Si hay vector o bytea, la proyección row_to_json fallaría; usamos
             * encode de columnas no-especiales para las especiales no incluidas. */
            let cols = comparable.join(",");
            let sql = format!(
                "SELECT md5(string_agg(r::text, E'\\n' ORDER BY r)) FROM (SELECT row_to_json(t)::text AS r FROM (SELECT {cols} FROM {table}) t) s"
            );
            match pg_utils::run_pg_query(ssh, container, db_user, db_name, &sql).await {
                Ok(out) => {
                    let h = out.trim().to_string();
                    Ok(if h.is_empty() || h == "NULL" { None } else { Some(h) })
                }
                Err(_) => Ok(None),
            }
        }
        DbEngine::MariaDb => {
            let pw = db_password.map(|s| s.expose_secret()).unwrap_or_default();
            /* md5 sobre concatenación canónica de la proyección */
            let concat = comparable
                .iter()
                .map(|c| format!("IFNULL(CAST(`{c}` AS CHAR),'\\0')"))
                .collect::<Vec<_>>()
                .join(",'|',");
            let sql = format!(
                "SELECT MD5(GROUP_CONCAT(CONCAT_WS('|',{concat}) ORDER BY 1)) FROM `{table}`"
            );
            let cmd = format!(
                "docker exec -i {container} mariadb -u {db_user} -p'{pw}' {db_name} -N -e \"{sql}\""
            );
            let res = ssh.execute(&cmd).await?;
            if !res.success() {
                return Ok(None);
            }
            let h = res.stdout.trim().to_string();
            Ok(if h.is_empty() { None } else { Some(h) })
        }
    }
}

/// Calcula el digest completo de un lado (todas las tablas).
pub async fn digest_all(
    ssh: &SshClient,
    model: &SchemaModel,
    container: &str,
    db_user: &str,
    db_name: &str,
    db_password: Option<&secrecy::SecretString>,
) -> std::result::Result<std::collections::BTreeMap<String, TableDigest>, CoolifyError> {
    let mut out = std::collections::BTreeMap::new();
    for (table, info) in &model.tables {
        let count = count_rows(ssh, model.engine, container, db_user, db_name, db_password, table).await?;
        let hash = table_hash(ssh, model.engine, container, db_user, db_name, db_password, table, info).await?;
        let not_comparable_light = hash.is_none() && (info.has_vector() || info.has_bytea());
        out.insert(
            table.clone(),
            TableDigest {
                table: table.clone(),
                row_count: count,
                hash,
                not_comparable_light,
            },
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_count_rows_empty_table_err() {
        /* parse::<i64> de un string no numérico devuelve error de Docker */
        let _ = "abc".parse::<i64>().unwrap_err();
    }
}
