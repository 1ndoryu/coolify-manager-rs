/* Split 119A-3 de restore_pg_data.rs — fases 0-4: helpers + mostrar_info/snapshot/preparar/convertir. */

use super::{
    BackupPreparado, CtxRestore, READINESS_POLL_SECS, READINESS_TIMEOUT_SECS, TEMP_POSTGRES_IMAGE,
};
use crate::domain::CommandOutput;
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use std::path::Path;

pub(super) async fn wait_for_postgres_ready(
    ssh: &SshClient,
    container: &str,
    timeout_secs: u64,
) -> Result<bool, CoolifyError> {
    let start = std::time::Instant::now();
    loop {
        let elapsed = start.elapsed().as_secs();
        if elapsed >= timeout_secs {
            return Ok(false);
        }

        let check = ssh
            .execute(&format!(
                "docker exec {container} pg_isready -U rust_app 2>&1"
            ))
            .await
            .unwrap_or(CommandOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 1,
            });

        if check.exit_code == 0 {
            /* pg_isready listo, pero verificar que acepta conexiones con una query real */
            let query_check = ssh
                .execute(&format!(
                    "docker exec {container} psql -U rust_app -d postgres -c 'SELECT 1;' 2>&1"
                ))
                .await
                .unwrap_or(CommandOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 1,
                });
            if query_check.exit_code == 0 {
                return Ok(true);
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(READINESS_POLL_SECS)).await;
    }
}

pub(super) async fn restore_safety_snapshot(
    ssh: &SshClient,
    postgres_container: &str,
    snapshot_path: &str,
    db_name: &str,
) {
    println!("   Restaurando safety snapshot desde {snapshot_path}...");

    /* Verificar que el snapshot existe y tiene contenido */
    let check = ssh
        .execute(&format!("test -s {snapshot_path} && echo OK || echo EMPTY"))
        .await
        .unwrap_or_default();
    if !check.stdout.contains("OK") {
        eprintln!("   ⚠ Safety snapshot vacío o inexistente — no se puede restaurar");
        return;
    }

    /* Copiar snapshot al contenedor */
    let container_path = "/tmp/safety_restore.sql";
    let _ = docker::copy_to_container(
        ssh,
        Path::new(snapshot_path),
        postgres_container,
        container_path,
    )
    .await;

    /* Drop + recreate + import */
    let _ = docker::docker_exec(
        ssh,
        postgres_container,
        &format!(
            "psql -U rust_app -d postgres -c 'DROP DATABASE IF EXISTS {db_name};' -c 'CREATE DATABASE {db_name} OWNER rust_app;'"
        ),
    )
    .await;

    let import = docker::docker_exec(
        ssh,
        postgres_container,
        &format!("psql -U rust_app -d {db_name} < {container_path}"),
    )
    .await;

    let _ = docker::docker_exec(ssh, postgres_container, &format!("rm -f {container_path}")).await;

    match import {
        Ok(r) if r.success() => println!("   ✓ Safety snapshot restaurado"),
        Ok(r) => eprintln!("   ✗ Error restaurando snapshot: {}", r.stderr.trim()),
        Err(e) => eprintln!("   ✗ Error restaurando snapshot: {e}"),
    }
}

pub(super) async fn cleanup_tmp(ssh: &SshClient, tmp_dir: &str, snapshot_path: &str) {
    let _ = ssh.execute(&format!("rm -rf {tmp_dir} 2>/dev/null")).await;
    /* No eliminar snapshot — es la red de seguridad */
    let _ = snapshot_path; /* suppress unused warning */
}

pub(super) fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

pub(super) fn rand_u16() -> u16 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    std::time::Instant::now().hash(&mut h);
    h.finish() as u16
}

/* ── Fase 1: mostrar resumen de lo validado ─────────────────── */
pub(super) fn fase_mostrar_info(ctx: &CtxRestore<'_>) {
    println!("═══ restore-pg-data ═══");
    println!("[1/7] Validando sitio y resolviendo containers...");
    println!("   Stack UUID:  {}", ctx.stack_uuid);
    println!("   Postgres:    {}", ctx.postgres_container);
    println!("   App:         {}", ctx.app_container);
    println!("   Database:    {}", ctx.db_name);
    println!("   Backup file: {}", ctx.file.display());
}

/* ── Fase 2: Safety snapshot ───────────────────────────────── */
pub(super) async fn fase_snapshot(ctx: &CtxRestore<'_>) -> std::result::Result<(), CoolifyError> {
    let db_name = ctx.db_name.as_str();
    let postgres_container = ctx.postgres_container.as_str();
    let snapshot_path = ctx.snapshot_path.as_str();

    if ctx.skip_safety_snapshot {
        println!("[2/7] Safety snapshot OMITIDO (--skip-safety-snapshot)");
        return Ok(());
    }

    println!("[2/7] Creando safety snapshot de la DB actual...");
    let dump_cmd = format!(
        "pg_dump -U rust_app -d {db_name} --clean --if-exists 2>/dev/null || echo 'EMPTY_DB'"
    );
    let dump_result = docker::docker_exec(ctx.ssh, postgres_container, &dump_cmd).await?;

    /* Guardar snapshot en el host (no en el contenedor) */
    let write_cmd = format!(
        "echo '{}' | base64 -d > {}",
        base64_encode(dump_result.stdout.as_bytes()),
        snapshot_path
    );
    ctx.ssh.execute(&write_cmd).await?;

    let snapshot_size = dump_result.stdout.len();
    if dump_result.stdout.contains("EMPTY_DB") || snapshot_size < 50 {
        println!("   DB actual vacía o sin tablas (seed fresco)");
    } else {
        println!("   Snapshot: {snapshot_path} ({snapshot_size} bytes)");
    }
    Ok(())
}

/* ── Fase 3: Upload + extraer tarball ──────────────────────── */
pub(super) async fn fase_preparar_backup(
    ctx: &CtxRestore<'_>,
) -> std::result::Result<BackupPreparado, CoolifyError> {
    let tmp_dir = ctx.tmp_dir.as_str();
    let snapshot_path = ctx.snapshot_path.as_str();
    let file = ctx.file;

    println!("[3/7] Preparando backup en el servidor...");
    let remote_tarball = format!("{tmp_dir}/data.tar.gz");

    ctx.ssh.execute(&format!("mkdir -p {tmp_dir}")).await?;

    /* Determinar si el archivo es local o ya está en el servidor */
    let is_remote = !file.exists();
    if is_remote {
        /* El archivo ya está en el VPS — solo verificar que existe */
        let remote_path = file.display().to_string();
        let check = ctx
            .ssh
            .execute(&format!(
                "test -f '{}' && echo EXISTS || echo MISSING",
                remote_path
            ))
            .await?;
        if !check.stdout.contains("EXISTS") {
            cleanup_tmp(ctx.ssh, tmp_dir, snapshot_path).await;
            return Err(CoolifyError::Validation(format!(
                "Archivo no encontrado en el servidor: {remote_path}"
            )));
        }
        /* Crear symlink o copia al tmp_dir */
        ctx.ssh
            .execute(&format!("cp '{}' '{}'", remote_path, remote_tarball))
            .await?;
        println!("   Archivo remoto copiado a {remote_tarball}");
    } else {
        /* Archivo local — upload streamed (soporta >2MB) */
        println!(
            "   Subiendo {} ({:.1} MB)...",
            file.display(),
            std::fs::metadata(file)?.len() as f64 / 1_048_576.0
        );
        ctx.ssh.upload_file_streamed(file, &remote_tarball).await?;
        println!("   Upload completado");
    }

    /* Extraer tarball */
    let extract_cmd = format!(
        "mkdir -p {tmp_dir}/data && tar xzf {remote_tarball} -C {tmp_dir}/data --strip-components=0 2>&1"
    );
    let extract_result = ctx.ssh.execute(&extract_cmd).await?;
    if !extract_result.success() {
        cleanup_tmp(ctx.ssh, tmp_dir, snapshot_path).await;
        return Err(CoolifyError::Validation(format!(
            "Error extrayendo tarball: {}",
            extract_result.stderr
        )));
    }

    /* Detectar PG_VERSION dentro del data directory */
    let detect_cmd = format!("find {tmp_dir}/data -name PG_VERSION -type f | head -1");
    let pg_version_file = ctx.ssh.execute(&detect_cmd).await?;
    let pg_version_path = pg_version_file.stdout.trim();
    if pg_version_path.is_empty() {
        cleanup_tmp(ctx.ssh, tmp_dir, snapshot_path).await;
        return Err(CoolifyError::Validation(
            "El tarball no contiene un data directory de PostgreSQL válido (no se encontró PG_VERSION)".to_string(),
        ));
    }
    /* Obtener directorio padre de PG_VERSION = data dir real */
    let data_dir = pg_version_path
        .rsplit_once('/')
        .map(|(d, _)| d)
        .unwrap_or(pg_version_path);
    let pg_version = ctx
        .ssh
        .execute(&format!("cat '{pg_version_path}'"))
        .await?
        .stdout
        .trim()
        .to_string();
    println!("   Extraído. PG version: {pg_version}, data dir: {data_dir}");

    Ok(BackupPreparado {
        data_dir: data_dir.to_string(),
    })
}

/* ── Fase 4: Postgres temporal + pg_dump ───────────────────── */
pub(super) async fn fase_convertir_sql(
    ctx: &CtxRestore<'_>,
    prep: &BackupPreparado,
) -> std::result::Result<String, CoolifyError> {
    let tmp_dir = ctx.tmp_dir.as_str();
    let snapshot_path = ctx.snapshot_path.as_str();
    let short_uid = ctx.short_uid.as_str();
    let db_name = ctx.db_name.as_str();

    println!("[4/7] Levantando postgres temporal para convertir a SQL...");
    let temp_name =
        levantar_postgres_temporal(ctx, prep, tmp_dir, snapshot_path, short_uid).await?;

    /* pg_dump desde el temporal */
    let sql_remote_path =
        extraer_dump_sql(ctx, tmp_dir, snapshot_path, short_uid, db_name, &temp_name).await?;

    /* Cleanup postgres temporal */
    let _ = ctx
        .ssh
        .execute(&format!("docker rm -f {temp_name} 2>/dev/null"))
        .await;
    println!("   Postgres temporal eliminado");

    Ok(sql_remote_path)
}

/* Copia el data dir, levanta el contenedor temporal y espera readiness. */
async fn levantar_postgres_temporal(
    ctx: &CtxRestore<'_>,
    prep: &BackupPreparado,
    tmp_dir: &str,
    snapshot_path: &str,
    short_uid: &str,
) -> std::result::Result<String, CoolifyError> {
    let temp_name = format!("cm-pgdata-{short_uid}");
    let temp_port = 15432 + (rand_u16() % 5000);

    /* Copiar data dir a ubicación writable (no modificar el original extraído) */
    let writable_data_dir = format!("{tmp_dir}/writable-data");
    let cp_cmd = format!("cp -a '{}' '{writable_data_dir}'", prep.data_dir);
    let cp_result = ctx.ssh.execute(&cp_cmd).await?;
    if !cp_result.success() {
        cleanup_tmp(ctx.ssh, tmp_dir, snapshot_path).await;
        let _ = ctx
            .ssh
            .execute(&format!("docker rm -f {temp_name} 2>/dev/null"))
            .await;
        return Err(CoolifyError::Validation(format!(
            "Error copiando data directory: {}",
            cp_result.stderr
        )));
    }

    /* Arreglar ownership para el usuario postgres (uid 999 en la imagen oficial) */
    ctx.ssh
        .execute(&format!(
            "chown -R 999:999 '{writable_data_dir}' 2>/dev/null"
        ))
        .await?;

    /* Levantar postgres temporal con el data directory writable */
    let run_cmd = format!(
        "docker run -d --name {temp_name} \
         -p {temp_port}:5432 \
         -e POSTGRES_HOST_AUTH_METHOD=trust \
         -e PGUSER=rust_app \
         -v '{writable_data_dir}:/var/lib/postgresql/data' \
         {TEMP_POSTGRES_IMAGE} 2>&1"
    );
    let run_result = ctx.ssh.execute(&run_cmd).await?;
    if !run_result.success() {
        cleanup_tmp(ctx.ssh, tmp_dir, snapshot_path).await;
        let _ = ctx
            .ssh
            .execute(&format!("docker rm -f {temp_name} 2>/dev/null"))
            .await;
        return Err(CoolifyError::Docker {
            exit_code: run_result.exit_code,
            stderr: format!("Error levantando postgres temporal: {}", run_result.stderr),
        });
    }

    /* Esperar readiness con timeout explícito */
    println!("   Esperando postgres temporal (timeout {READINESS_TIMEOUT_SECS}s)...");
    let ready = wait_for_postgres_ready(ctx.ssh, &temp_name, READINESS_TIMEOUT_SECS).await?;
    if !ready {
        let logs = ctx
            .ssh
            .execute(&format!("docker logs {temp_name} 2>&1 | tail -20"))
            .await
            .unwrap_or_default();
        let _ = ctx
            .ssh
            .execute(&format!("docker rm -f {temp_name} 2>/dev/null"))
            .await;
        cleanup_tmp(ctx.ssh, tmp_dir, snapshot_path).await;
        return Err(CoolifyError::Validation(format!(
            "Postgres temporal no alcanzó readiness. Logs:\n{}",
            logs.stdout
        )));
    }
    println!("   Postgres temporal listo");
    Ok(temp_name)
}

/* pg_dump desde el temporal + subida streamed del SQL al host. */
async fn extraer_dump_sql(
    ctx: &CtxRestore<'_>,
    tmp_dir: &str,
    snapshot_path: &str,
    short_uid: &str,
    db_name: &str,
    temp_name: &str,
) -> std::result::Result<String, CoolifyError> {
    /* pg_dump desde el temporal */
    let dump_cmd = format!(
        "docker exec {temp_name} pg_dump -U rust_app -d {db_name} --clean --if-exists 2>&1"
    );
    let dump_result = ctx.ssh.execute(&dump_cmd).await?;
    if !dump_result.success() || dump_result.stdout.trim().is_empty() {
        /* Intentar listar DBs disponibles */
        let list_dbs = ctx
            .ssh
            .execute(&format!("docker exec {temp_name} psql -U rust_app -l 2>&1"))
            .await
            .unwrap_or_default();
        let _ = ctx
            .ssh
            .execute(&format!("docker rm -f {temp_name} 2>/dev/null"))
            .await;
        cleanup_tmp(ctx.ssh, tmp_dir, snapshot_path).await;
        return Err(CoolifyError::Validation(format!(
            "pg_dump falló en postgres temporal. DBs disponibles:\n{}\nError: {}",
            list_dbs.stdout, dump_result.stderr
        )));
    }

    let sql_dump = dump_result.stdout;
    let sql_size = sql_dump.len();
    println!("   Dump generado: {sql_size} bytes");

    /* Guardar SQL temporalmente en el host para poder copiarlo al postgres de producción.
     * Siempre usamos streamed upload — el base64 via echo falla con "Argument list too long"
     * porque el dump SQL codificado excede ARG_MAX del shell. */
    let sql_remote_path = format!("{tmp_dir}/dump.sql");
    /* [119A-5] canonicalize: temporal generado desde short_uid (uuid hex). */
    let tmp_name = format!("cm-dump-{short_uid}.sql");
    validation::validar_segmento_ruta(&tmp_name, "temporal")?;
    let sql_local = validation::join_segmento_seguro(&std::env::temp_dir(), &tmp_name, "temporal")?;
    std::fs::write(&sql_local, sql_dump.as_bytes())?;
    ctx.ssh
        .upload_file_streamed(&sql_local, &sql_remote_path)
        .await?;
    let _ = std::fs::remove_file(&sql_local);

    Ok(sql_remote_path)
}

/* ── Fase 5: Parar app ─────────────────────────────────────── */
