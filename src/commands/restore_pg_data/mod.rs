/*
 * restore-pg-data — Restaura un data directory raw de PostgreSQL en un sitio existente.
 *
 * Acepta un tarball de un data directory de PG16 (no un pg_dump SQL).
 * Flujo: extraer → postgres temporal → pg_dump → parar app → drop+recreate DB → import → cleanup.
 *
 * Cada fase valida exit codes y limpia recursos incluso en error.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;

use std::path::Path;
use uuid::Uuid;

mod aplicacion;
mod preparacion;
use aplicacion::*;
use preparacion::*;

const TEMP_POSTGRES_IMAGE: &str = "postgres:16";
const READINESS_TIMEOUT_SECS: u64 = 60;
const READINESS_POLL_SECS: u64 = 2;

/* [119A-3] Contexto compartido por las 7 fases de la restauración.
 * Evita pasar 7+ parámetros a cada fase (clippy::too-many-arguments). */
struct CtxRestore<'a> {
    ssh: &'a SshClient,
    site_name: &'a str,
    file: &'a Path,
    stack_uuid: &'a str,
    postgres_container: String,
    app_container: String,
    db_name: String,
    tmp_dir: String,
    snapshot_path: String,
    short_uid: String,
    skip_safety_snapshot: bool,
}

/* [119A-3] Resultado de la fase 3: data dir real detectado. */
struct BackupPreparado {
    data_dir: String,
}

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    file: &Path,
    database: Option<&str>,
    skip_safety_snapshot: bool,
) -> std::result::Result<(), CoolifyError> {
    /* ── Fase 1: Validación ────────────────────────────────────── */
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    let stack_uuid = site.stack_uuid.as_deref().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{site_name}' no tiene stack_uuid"))
    })?;

    let target = settings.resolve_site_target(site)?;
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let postgres_container = docker::find_postgres_container(&ssh, stack_uuid).await?;
    let app_container = docker::find_app_container(&ssh, stack_uuid).await?;

    let uid = Uuid::new_v4().to_string();
    let ctx = CtxRestore {
        ssh: &ssh,
        site_name,
        file,
        stack_uuid,
        postgres_container,
        app_container,
        db_name: database.unwrap_or("rust_db").to_string(),
        tmp_dir: format!("/tmp/cm-pgdata-{}", &uid[..8]),
        snapshot_path: format!("/tmp/cm-safety-{}.sql", Uuid::new_v4()),
        short_uid: uid[..8].to_string(),
        skip_safety_snapshot,
    };

    fase_mostrar_info(&ctx);

    fase_snapshot(&ctx).await?;

    let prep = fase_preparar_backup(&ctx).await?;

    let sql_remote_path = fase_convertir_sql(&ctx, &prep).await?;

    fase_parar_app(&ctx).await?;

    let table_count = fase_restaurar(&ctx, &sql_remote_path).await?;

    fase_finalizar(&ctx, table_count).await?;
    Ok(())
}

/* ── Helpers ───────────────────────────────────────────────────── */
