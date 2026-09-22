/* Split 119A-3 de restore_pg_data.rs — fases 5-7: parar_app/restaurar/finalizar. */

use super::preparacion::{cleanup_tmp, restore_safety_snapshot};
use super::CtxRestore;
use crate::error::CoolifyError;
use crate::infra::docker;

pub(super) async fn fase_parar_app(ctx: &CtxRestore<'_>) -> std::result::Result<(), CoolifyError> {
    let app_container = ctx.app_container.as_str();

    println!("[5/7] Parando app para evitar writes...");
    let stop_result = ctx
        .ssh
        .execute(&format!("docker stop {app_container} 2>&1"))
        .await?;
    if !stop_result.success() {
        /* App puede ya estar parada — no es error fatal */
        println!(
            "   ⚠ App ya estaba parada o no se pudo parar: {}",
            stop_result.stderr.trim()
        );
    } else {
        println!("   App parada");
    }
    Ok(())
}

/* ── Fase 6: Restaurar ─────────────────────────────────────── */
pub(super) async fn fase_restaurar(
    ctx: &CtxRestore<'_>,
    sql_remote_path: &str,
) -> std::result::Result<i32, CoolifyError> {
    let postgres_container = ctx.postgres_container.as_str();
    let app_container = ctx.app_container.as_str();
    let snapshot_path = ctx.snapshot_path.as_str();
    let tmp_dir = ctx.tmp_dir.as_str();
    let db_name = ctx.db_name.as_str();

    println!("[6/7] Restaurando base de datos...");

    /* Copiar SQL al contenedor postgres de producción.
     * El archivo ya está en el host remoto, usamos docker cp via SSH directo. */
    let container_sql_path = "/tmp/restore.sql";
    let cp_cmd =
        format!("docker cp '{sql_remote_path}' '{postgres_container}:{container_sql_path}' 2>&1");
    let cp_result = ctx.ssh.execute(&cp_cmd).await?;
    if !cp_result.success() {
        if !ctx.skip_safety_snapshot {
            restore_safety_snapshot(ctx.ssh, postgres_container, snapshot_path, db_name).await;
        }
        let _ = ctx
            .ssh
            .execute(&format!("docker start {app_container} 2>/dev/null"))
            .await;
        cleanup_tmp(ctx.ssh, tmp_dir, snapshot_path).await;
        return Err(CoolifyError::Validation(format!(
            "Error copiando SQL al contenedor postgres: {}",
            cp_result.stderr
        )));
    }
    println!("   SQL copiado al contenedor postgres");

    /* Drop + recreate DB para limpieza total */
    let drop_cmd = format!(
        "psql -U rust_app -d postgres -c \"SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{db_name}' AND pid <> pg_backend_pid();\" 2>&1"
    );
    let _ = docker::docker_exec(ctx.ssh, postgres_container, &drop_cmd).await;

    let recreate_cmd = format!(
        "psql -U rust_app -d postgres -c 'DROP DATABASE IF EXISTS {db_name};' -c 'CREATE DATABASE {db_name} OWNER rust_app;' 2>&1"
    );
    let recreate_result = docker::docker_exec(ctx.ssh, postgres_container, &recreate_cmd).await?;
    if !recreate_result.success() {
        /* Intentar restaurar safety snapshot */
        if !ctx.skip_safety_snapshot {
            restore_safety_snapshot(ctx.ssh, postgres_container, snapshot_path, db_name).await;
        }
        let _ = ctx
            .ssh
            .execute(&format!("docker start {app_container} 2>/dev/null"))
            .await;
        cleanup_tmp(ctx.ssh, tmp_dir, snapshot_path).await;
        return Err(CoolifyError::Docker {
            exit_code: recreate_result.exit_code,
            stderr: format!("Error recreando DB: {}", recreate_result.stderr),
        });
    }

    /* Importar SQL via psql dentro del contenedor */
    let import_cmd = format!("psql -U rust_app -d {db_name} < {container_sql_path} 2>&1");
    let import_result = docker::docker_exec(ctx.ssh, postgres_container, &import_cmd).await?;

    /* Limpiar SQL del contenedor */
    let _ = docker::docker_exec(
        ctx.ssh,
        postgres_container,
        &format!("rm -f {container_sql_path}"),
    )
    .await;

    if !import_result.success() {
        eprintln!("   ✗ Error importando SQL: {}", import_result.stderr.trim());
        if !ctx.skip_safety_snapshot {
            println!("   Restaurando safety snapshot...");
            restore_safety_snapshot(ctx.ssh, postgres_container, snapshot_path, db_name).await;
        }
        let _ = ctx
            .ssh
            .execute(&format!("docker start {app_container} 2>/dev/null"))
            .await;
        cleanup_tmp(ctx.ssh, tmp_dir, snapshot_path).await;
        return Err(CoolifyError::Docker {
            exit_code: import_result.exit_code,
            stderr: format!("Error importando SQL: {}", import_result.stderr),
        });
    }

    /* Verificar que hay datos */
    let verify_cmd = format!(
        "psql -U rust_app -d {db_name} -t -c \"SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public';\" 2>&1"
    );
    let verify_result = docker::docker_exec(ctx.ssh, postgres_container, &verify_cmd).await?;
    let table_count = verify_result.stdout.trim().parse::<i32>().unwrap_or(0);
    println!("   ✓ Importado. {table_count} tablas en {db_name}");

    Ok(table_count)
}

/* ── Fase 7: Levantar app + cleanup ────────────────────────── */
pub(super) async fn fase_finalizar(
    ctx: &CtxRestore<'_>,
    table_count: i32,
) -> std::result::Result<(), CoolifyError> {
    let app_container = ctx.app_container.as_str();
    let tmp_dir = ctx.tmp_dir.as_str();
    let snapshot_path = ctx.snapshot_path.as_str();
    let db_name = ctx.db_name.as_str();
    let site_name = ctx.site_name;

    println!("[7/7] Levantando app y limpiando...");
    let start_result = ctx
        .ssh
        .execute(&format!("docker start {app_container} 2>&1"))
        .await?;
    if !start_result.success() {
        eprintln!(
            "   ⚠ No se pudo levantar app automáticamente: {}",
            start_result.stderr.trim()
        );
        eprintln!("   Levanta manualmente: docker start {app_container}");
    } else {
        println!("   ✓ App levantada");
    }

    cleanup_tmp(ctx.ssh, tmp_dir, snapshot_path).await;

    println!();
    println!("═══ Restauración completada ═══");
    println!("   Sitio:     {site_name}");
    println!("   Database:  {db_name}");
    println!("   Tablas:    {table_count}");
    if !ctx.skip_safety_snapshot {
        println!("   Snapshot:  {snapshot_path} (conservado por seguridad)");
        println!("   Para eliminar: ssh al servidor y rm {snapshot_path}");
    }
    Ok(())
}
