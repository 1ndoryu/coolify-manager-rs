/*
 * restore-pg-data — Restaura un data directory raw de PostgreSQL en un sitio existente.
 *
 * Acepta un tarball de un data directory de PG16 (no un pg_dump SQL).
 * Flujo: extraer → postgres temporal → pg_dump → parar app → drop+recreate DB → import → cleanup.
 *
 * Cada fase valida exit codes y limpia recursos incluso en error.
 */

use crate::config::Settings;
use crate::domain::CommandOutput;
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;

use std::path::Path;
use uuid::Uuid;

const TEMP_POSTGRES_IMAGE: &str = "postgres:16";
const READINESS_TIMEOUT_SECS: u64 = 60;
const READINESS_POLL_SECS: u64 = 2;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    file: &Path,
    database: Option<&str>,
    skip_safety_snapshot: bool,
) -> std::result::Result<(), CoolifyError> {
    /* ── Fase 1: Validación ────────────────────────────────────── */
    println!("═══ restore-pg-data ═══");
    println!("[1/7] Validando sitio y resolviendo containers...");

    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    let stack_uuid = site
        .stack_uuid
        .as_deref()
        .ok_or_else(|| CoolifyError::Validation(format!("Sitio '{site_name}' no tiene stack_uuid")))?;

    let target = settings.resolve_site_target(site)?;
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let postgres_container = docker::find_postgres_container(&ssh, stack_uuid).await?;
    let app_container = docker::find_app_container(&ssh, stack_uuid).await?;
    let db_name = database.unwrap_or("rust_db");

    println!("   Stack UUID:  {stack_uuid}");
    println!("   Postgres:    {postgres_container}");
    println!("   App:         {app_container}");
    println!("   Database:    {db_name}");
    println!("   Backup file: {}", file.display());

    /* ── Fase 2: Safety snapshot ───────────────────────────────── */
    let snapshot_path = format!("/tmp/cm-safety-{}.sql", Uuid::new_v4());

    if skip_safety_snapshot {
        println!("[2/7] Safety snapshot OMITIDO (--skip-safety-snapshot)");
    } else {
        println!("[2/7] Creando safety snapshot de la DB actual...");
        let dump_cmd = format!(
            "pg_dump -U rust_app -d {db_name} --clean --if-exists 2>/dev/null || echo 'EMPTY_DB'"
        );
        let dump_result = docker::docker_exec(&ssh, &postgres_container, &dump_cmd).await?;

        /* Guardar snapshot en el host (no en el contenedor) */
        let write_cmd = format!(
            "echo '{}' | base64 -d > {}",
            base64_encode(dump_result.stdout.as_bytes()),
            snapshot_path
        );
        ssh.execute(&write_cmd).await?;

        let snapshot_size = dump_result.stdout.len();
        if dump_result.stdout.contains("EMPTY_DB") || snapshot_size < 50 {
            println!("   DB actual vacía o sin tablas (seed fresco)");
        } else {
            println!("   Snapshot: {snapshot_path} ({snapshot_size} bytes)");
        }
    }

    /* ── Fase 3: Upload + extraer tarball ──────────────────────── */
    println!("[3/7] Preparando backup en el servidor...");
    let uid = Uuid::new_v4().to_string();
    let short_uid = &uid[..8];
    let tmp_dir = format!("/tmp/cm-pgdata-{short_uid}");
    let remote_tarball = format!("{tmp_dir}/data.tar.gz");

    ssh.execute(&format!("mkdir -p {tmp_dir}")).await?;

    /* Determinar si el archivo es local o ya está en el servidor */
    let is_remote = !file.exists();
    if is_remote {
        /* El archivo ya está en el VPS — solo verificar que existe */
        let remote_path = file.display().to_string();
        let check = ssh
            .execute(&format!("test -f '{}' && echo EXISTS || echo MISSING", remote_path))
            .await?;
        if !check.stdout.contains("EXISTS") {
            cleanup_tmp(&ssh, &tmp_dir, &snapshot_path).await;
            return Err(CoolifyError::Validation(format!(
                "Archivo no encontrado en el servidor: {remote_path}"
            )));
        }
        /* Crear symlink o copia al tmp_dir */
        ssh.execute(&format!("cp '{}' '{}'", remote_path, remote_tarball))
            .await?;
        println!("   Archivo remoto copiado a {remote_tarball}");
    } else {
        /* Archivo local — upload streamed (soporta >2MB) */
        println!(
            "   Subiendo {} ({:.1} MB)...",
            file.display(),
            std::fs::metadata(file)?.len() as f64 / 1_048_576.0
        );
        ssh.upload_file_streamed(file, &remote_tarball).await?;
        println!("   Upload completado");
    }

    /* Extraer tarball */
    let extract_cmd = format!(
        "mkdir -p {tmp_dir}/data && tar xzf {remote_tarball} -C {tmp_dir}/data --strip-components=0 2>&1"
    );
    let extract_result = ssh.execute(&extract_cmd).await?;
    if !extract_result.success() {
        cleanup_tmp(&ssh, &tmp_dir, &snapshot_path).await;
        return Err(CoolifyError::Validation(format!(
            "Error extrayendo tarball: {}",
            extract_result.stderr
        )));
    }

    /* Detectar PG_VERSION dentro del data directory */
    let detect_cmd = format!(
        "find {tmp_dir}/data -name PG_VERSION -type f | head -1"
    );
    let pg_version_file = ssh.execute(&detect_cmd).await?;
    let pg_version_path = pg_version_file.stdout.trim();
    if pg_version_path.is_empty() {
        cleanup_tmp(&ssh, &tmp_dir, &snapshot_path).await;
        return Err(CoolifyError::Validation(
            "El tarball no contiene un data directory de PostgreSQL válido (no se encontró PG_VERSION)".to_string(),
        ));
    }
    /* Obtener directorio padre de PG_VERSION = data dir real */
    let data_dir = pg_version_path.rsplit_once('/').map(|(d, _)| d).unwrap_or(pg_version_path);
    let pg_version = ssh
        .execute(&format!("cat '{pg_version_path}'"))
        .await?
        .stdout
        .trim()
        .to_string();
    println!("   Extraído. PG version: {pg_version}, data dir: {data_dir}");

    /* ── Fase 4: Postgres temporal + pg_dump ───────────────────── */
    println!("[4/7] Levantando postgres temporal para convertir a SQL...");
    let temp_name = format!("cm-pgdata-{short_uid}");
    let temp_port = 15432 + (rand_u16() % 5000);

    /* Copiar data dir a ubicación writable (no modificar el original extraído) */
    let writable_data_dir = format!("{tmp_dir}/writable-data");
    let cp_cmd = format!("cp -a '{data_dir}' '{writable_data_dir}'");
    let cp_result = ssh.execute(&cp_cmd).await?;
    if !cp_result.success() {
        cleanup_tmp(&ssh, &tmp_dir, &snapshot_path).await;
        let _ = ssh.execute(&format!("docker rm -f {temp_name} 2>/dev/null")).await;
        return Err(CoolifyError::Validation(format!(
            "Error copiando data directory: {}", cp_result.stderr
        )));
    }

    /* Arreglar ownership para el usuario postgres (uid 999 en la imagen oficial) */
    ssh.execute(&format!("chown -R 999:999 '{writable_data_dir}' 2>/dev/null")).await?;

    /* Levantar postgres temporal con el data directory writable */
    let run_cmd = format!(
        "docker run -d --name {temp_name} \
         -p {temp_port}:5432 \
         -e POSTGRES_HOST_AUTH_METHOD=trust \
         -e PGUSER=rust_app \
         -v '{writable_data_dir}:/var/lib/postgresql/data' \
         {TEMP_POSTGRES_IMAGE} 2>&1"
    );
    let run_result = ssh.execute(&run_cmd).await?;
    if !run_result.success() {
        cleanup_tmp(&ssh, &tmp_dir, &snapshot_path).await;
        let _ = ssh
            .execute(&format!("docker rm -f {temp_name} 2>/dev/null"))
            .await;
        return Err(CoolifyError::Docker {
            exit_code: run_result.exit_code,
            stderr: format!("Error levantando postgres temporal: {}", run_result.stderr),
        });
    }

    /* Esperar readiness con timeout explícito */
    println!("   Esperando postgres temporal (timeout {READINESS_TIMEOUT_SECS}s)...");
    let ready = wait_for_postgres_ready(&ssh, &temp_name, READINESS_TIMEOUT_SECS).await?;
    if !ready {
        let logs = ssh
            .execute(&format!("docker logs {temp_name} 2>&1 | tail -20"))
            .await
            .unwrap_or_default();
        let _ = ssh
            .execute(&format!("docker rm -f {temp_name} 2>/dev/null"))
            .await;
        cleanup_tmp(&ssh, &tmp_dir, &snapshot_path).await;
        return Err(CoolifyError::Validation(format!(
            "Postgres temporal no alcanzó readiness. Logs:\n{}",
            logs.stdout
        )));
    }
    println!("   Postgres temporal listo");

    /* pg_dump desde el temporal */
    let dump_cmd = format!(
        "docker exec {temp_name} pg_dump -U rust_app -d {db_name} --clean --if-exists 2>&1"
    );
    let dump_result = ssh.execute(&dump_cmd).await?;
    if !dump_result.success() || dump_result.stdout.trim().is_empty() {
        /* Intentar listar DBs disponibles */
        let list_dbs = ssh
            .execute(&format!(
                "docker exec {temp_name} psql -U rust_app -l 2>&1"
            ))
            .await
            .unwrap_or_default();
        let _ = ssh
            .execute(&format!("docker rm -f {temp_name} 2>/dev/null"))
            .await;
        cleanup_tmp(&ssh, &tmp_dir, &snapshot_path).await;
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
    let sql_local = std::env::temp_dir().join(format!("cm-dump-{short_uid}.sql"));
    std::fs::write(&sql_local, sql_dump.as_bytes())?;
    ssh.upload_file_streamed(&sql_local, &sql_remote_path).await?;
    let _ = std::fs::remove_file(&sql_local);

    /* Cleanup postgres temporal */
    let _ = ssh
        .execute(&format!("docker rm -f {temp_name} 2>/dev/null"))
        .await;
    println!("   Postgres temporal eliminado");

    /* ── Fase 5: Parar app ─────────────────────────────────────── */
    println!("[5/7] Parando app para evitar writes...");
    let stop_result = ssh
        .execute(&format!("docker stop {app_container} 2>&1"))
        .await?;
    if !stop_result.success() {
        /* App puede ya estar parada — no es error fatal */
        println!("   ⚠ App ya estaba parada o no se pudo parar: {}", stop_result.stderr.trim());
    } else {
        println!("   App parada");
    }

    /* ── Fase 6: Restaurar ─────────────────────────────────────── */
    println!("[6/7] Restaurando base de datos...");

    /* Copiar SQL al contenedor postgres de producción.
     * El archivo ya está en el host remoto, usamos docker cp via SSH directo. */
    let container_sql_path = "/tmp/restore.sql";
    let cp_cmd = format!(
        "docker cp '{sql_remote_path}' '{postgres_container}:{container_sql_path}' 2>&1"
    );
    let cp_result = ssh.execute(&cp_cmd).await?;
    if !cp_result.success() {
        if !skip_safety_snapshot {
            restore_safety_snapshot(&ssh, &postgres_container, &snapshot_path, db_name).await;
        }
        let _ = ssh
            .execute(&format!("docker start {app_container} 2>/dev/null"))
            .await;
        cleanup_tmp(&ssh, &tmp_dir, &snapshot_path).await;
        return Err(CoolifyError::Validation(format!(
            "Error copiando SQL al contenedor postgres: {}", cp_result.stderr
        )));
    }
    println!("   SQL copiado al contenedor postgres");

    /* Drop + recreate DB para limpieza total */
    let drop_cmd = format!(
        "psql -U rust_app -d postgres -c \"SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{db_name}' AND pid <> pg_backend_pid();\" 2>&1"
    );
    let _ = docker::docker_exec(&ssh, &postgres_container, &drop_cmd).await;

    let recreate_cmd = format!(
        "psql -U rust_app -d postgres -c 'DROP DATABASE IF EXISTS {db_name};' -c 'CREATE DATABASE {db_name} OWNER rust_app;' 2>&1"
    );
    let recreate_result = docker::docker_exec(&ssh, &postgres_container, &recreate_cmd).await?;
    if !recreate_result.success() {
        /* Intentar restaurar safety snapshot */
        if !skip_safety_snapshot {
            restore_safety_snapshot(&ssh, &postgres_container, &snapshot_path, db_name).await;
        }
        let _ = ssh
            .execute(&format!("docker start {app_container} 2>/dev/null"))
            .await;
        cleanup_tmp(&ssh, &tmp_dir, &snapshot_path).await;
        return Err(CoolifyError::Docker {
            exit_code: recreate_result.exit_code,
            stderr: format!("Error recreando DB: {}", recreate_result.stderr),
        });
    }

    /* Importar SQL via psql dentro del contenedor */
    let import_cmd = format!("psql -U rust_app -d {db_name} < {container_sql_path} 2>&1");
    let import_result = docker::docker_exec(&ssh, &postgres_container, &import_cmd).await?;

    /* Limpiar SQL del contenedor */
    let _ = docker::docker_exec(&ssh, &postgres_container, &format!("rm -f {container_sql_path}")).await;

    if !import_result.success() {
        eprintln!("   ✗ Error importando SQL: {}", import_result.stderr.trim());
        if !skip_safety_snapshot {
            println!("   Restaurando safety snapshot...");
            restore_safety_snapshot(&ssh, &postgres_container, &snapshot_path, db_name).await;
        }
        let _ = ssh
            .execute(&format!("docker start {app_container} 2>/dev/null"))
            .await;
        cleanup_tmp(&ssh, &tmp_dir, &snapshot_path).await;
        return Err(CoolifyError::Docker {
            exit_code: import_result.exit_code,
            stderr: format!("Error importando SQL: {}", import_result.stderr),
        });
    }

    /* Verificar que hay datos */
    let verify_cmd = format!(
        "psql -U rust_app -d {db_name} -t -c \"SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public';\" 2>&1"
    );
    let verify_result = docker::docker_exec(&ssh, &postgres_container, &verify_cmd).await?;
    let table_count = verify_result.stdout.trim().parse::<i32>().unwrap_or(0);
    println!("   ✓ Importado. {table_count} tablas en {db_name}");

    /* ── Fase 7: Levantar app + cleanup ────────────────────────── */
    println!("[7/7] Levantando app y limpiando...");
    let start_result = ssh
        .execute(&format!("docker start {app_container} 2>&1"))
        .await?;
    if !start_result.success() {
        eprintln!("   ⚠ No se pudo levantar app automáticamente: {}", start_result.stderr.trim());
        eprintln!("   Levanta manualmente: docker start {app_container}");
    } else {
        println!("   ✓ App levantada");
    }

    cleanup_tmp(&ssh, &tmp_dir, &snapshot_path).await;

    println!();
    println!("═══ Restauración completada ═══");
    println!("   Sitio:     {site_name}");
    println!("   Database:  {db_name}");
    println!("   Tablas:    {table_count}");
    if !skip_safety_snapshot {
        println!("   Snapshot:  {snapshot_path} (conservado por seguridad)");
        println!("   Para eliminar: ssh al servidor y rm {snapshot_path}");
    }
    Ok(())
}

/* ── Helpers ───────────────────────────────────────────────────── */

async fn wait_for_postgres_ready(
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

async fn restore_safety_snapshot(
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

    let _ = docker::docker_exec(
        ssh,
        postgres_container,
        &format!("rm -f {container_path}"),
    )
    .await;

    match import {
        Ok(r) if r.success() => println!("   ✓ Safety snapshot restaurado"),
        Ok(r) => eprintln!("   ✗ Error restaurando snapshot: {}", r.stderr.trim()),
        Err(e) => eprintln!("   ✗ Error restaurando snapshot: {e}"),
    }
}

async fn cleanup_tmp(ssh: &SshClient, tmp_dir: &str, snapshot_path: &str) {
    let _ = ssh
        .execute(&format!("rm -rf {tmp_dir} 2>/dev/null"))
        .await;
    /* No eliminar snapshot — es la red de seguridad */
    let _ = snapshot_path; /* suppress unused warning */
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn rand_u16() -> u16 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    std::time::Instant::now().hash(&mut h);
    h.finish() as u16
}
