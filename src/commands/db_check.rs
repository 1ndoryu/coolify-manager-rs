/*
 * Comando: db-check
 * Diagnostica la salud de la base de datos PostgreSQL de un sitio.
 * Verifica tablas existentes, estado de migraciones, y tablas/columnas esperadas.
 *
 * No modifica nada — es puramente diagnóstico.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::pg_utils;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    expected_tables: Option<&str>,
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

    println!("[db-check] {} — PostgreSQL health diagnostic", site_name);
    println!("  Database: {} @ {}", db_name, pg_container);
    println!();

    /* 1. Contar tablas */
    let table_count_sql = "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public';";
    let count = pg_utils::run_pg_query(&ssh, &pg_container, &db_user, &db_name, table_count_sql).await?;
    println!("  📊 {} tablas en public schema", count.trim());

    /* 2. Contar migraciones aplicadas */
    let migrations_sql = "SELECT count(*) FROM _sqlx_migrations;";
    let migration_count = pg_utils::run_pg_query(&ssh, &pg_container, &db_user, &db_name, migrations_sql).await?;
    println!("  📋 {} migraciones registradas en _sqlx_migrations", migration_count.trim());
    println!();

    /* 3. Verificar tablas esperadas */
    if let Some(expected) = expected_tables {
        let tables: Vec<&str> = expected.split(',').map(|t| t.trim()).collect();
        let mut issues = 0u32;

        println!("  Tablas esperadas:");
        for table in &tables {
            /* [db-check] Validar nombre de tabla contra SQL injection */
            if let Err(e) = pg_utils::validate_table_name(table) {
                println!("    ⚠️  {} — {}", table, e);
                issues += 1;
                continue;
            }
            let check_sql = format!(
                "SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_schema = 'public' AND table_name = '{}');",
                table
            );
            let exists = pg_utils::run_pg_query(&ssh, &pg_container, &db_user, &db_name, &check_sql).await?;
            let exists = exists.trim();
            if exists == "t" {
                println!("    ✅ {}", table);
            } else {
                println!("    ❌ {} — MISSING", table);
                issues += 1;
            }
        }
        println!();

        if issues > 0 {
            println!("  ⚠️  {} tablas faltantes — ejecuta db-migrate para aplicar", issues);
        } else {
            println!("  ✅ Todas las tablas esperadas existen");
        }
    }

    /* 4. Listar todas las tablas existentes */
    let tables_sql = "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public' ORDER BY table_name;";
    let tables = pg_utils::run_pg_query(&ssh, &pg_container, &db_user, &db_name, tables_sql).await?;
    println!();
    println!("  Tablas existentes:");
    for line in tables.lines() {
        let t = line.trim();
        if !t.is_empty() && t != "table_name" && !t.chars().all(|c| c == '-') {
            println!("    • {}", t);
        }
    }

    Ok(())
}
