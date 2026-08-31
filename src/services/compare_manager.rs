/*
 * compare_manager — orquesta la comparación de bases de datos (E12).
 *
 * Flujo:
 *   1. Resolver motor y credenciales del sitio vivo (PG o MariaDB).
 *   2. Descubrir esquema de la BD viva (automático, sin hardcodear tablas).
 *   3. Según el objetivo:
 *      - dump VPS/local → modo ligero (conteos+hash) o modo completo
 *        (restaura en contenedor temporal efímero y compara con SQL real).
 *      - otro sitio en vivo → compara las dos BDs directamente.
 *   4. Producir reporte JSON estable.
 *
 * Garantías: SOLO LECTURA sobre la BD viva; contenedor temporal SIEMPRE
 * limpiado; nombres de tablas validados; secrets nunca en el reporte.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::db_tmp;
use crate::infra::docker;
use crate::infra::pg_utils;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::compare::diff::{compare_table, TableDiff};
use crate::services::compare::digest::{digest_all, TableDigest};
use crate::services::compare::report::CompareReport;
use crate::services::compare::schema::{discover_mariadb, discover_postgres, DbEngine, SchemaModel};

use secrecy::{ExposeSecret, SecretString};
use std::collections::BTreeMap;
use std::path::Path;

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
struct SideCreds {
    engine: DbEngine,
    container: String,
    db_user: String,
    db_name: String,
    db_password: Option<SecretString>,
}

/// Resuelve credenciales de la BD viva de un sitio.
async fn resolve_live_creds(
    ssh: &SshClient,
    stack_uuid: &str,
) -> std::result::Result<SideCreds, CoolifyError> {
    /* Primero probar PostgreSQL (stacks Rust/kamples) */
    match docker::find_postgres_container(ssh, stack_uuid).await {
        Ok(pg_container) => {
            let (db_user, db_name) = {
                let app = docker::find_app_container(ssh, stack_uuid).await?;
                let url = docker::docker_exec(ssh, &app, "printenv DATABASE_URL").await?.stdout;
                let url = url.trim().to_string();
                if url.is_empty() {
                    /* Kamples usa KAMPLES_PG_* */
                    let (db, user, pass) = crate::services::database_manager::resolve_postgres_credentials(
                        ssh, &app,
                    )
                    .await?;
                    let _ = pass;
                    (user, db)
                } else {
                    pg_utils::parse_pg_credentials(&url)?
                }
            };
            Ok(SideCreds {
                engine: DbEngine::Postgres,
                container: pg_container,
                db_user,
                db_name,
                db_password: None,
            })
        }
        Err(_) => {
            /* Fallback: MariaDB/WordPress */
            let wp = docker::find_wordpress_container(ssh, stack_uuid).await?;
            let (db_name, db_user, db_password) =
                crate::services::database_manager::resolve_wordpress_credentials(ssh, &wp).await?;
            Ok(SideCreds {
                engine: DbEngine::MariaDb,
                container: docker::find_mariadb_container(ssh, stack_uuid).await?,
                db_user,
                db_name,
                db_password: Some(db_password),
            })
        }
    }
}

/// Descubre el esquema de un lado.
async fn discover_schema(
    ssh: &SshClient,
    creds: &SideCreds,
) -> std::result::Result<SchemaModel, CoolifyError> {
    match creds.engine {
        DbEngine::Postgres => {
            discover_postgres(ssh, &creds.container, &creds.db_user, &creds.db_name).await
        }
        DbEngine::MariaDb => {
            let pw = creds.db_password.as_ref().ok_or_else(|| {
                CoolifyError::Validation("MariaDB sin password".into())
            })?;
            discover_mariadb(ssh, &creds.container, &creds.db_name, &creds.db_user, pw).await
        }
    }
}

/// Ejecuta la comparación completa.
pub async fn execute(
    config_path: &Path,
    opts: &CompareOptions,
) -> std::result::Result<CompareReport, CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(&opts.site_name)?;
    validation::assert_site_ready(site)?;
    let stack_uuid = site.stack_uuid.as_deref().unwrap_or_default();
    let target = settings.resolve_site_target(site)?;

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let live = resolve_live_creds(&ssh, stack_uuid).await?;
    let live_schema = discover_schema(&ssh, &live).await?;

    /* Aplicar filtro de tablas */
    let table_filter: Option<Vec<String>> = opts.tables.as_ref().map(|t| {
        t.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    });

    /* ── Modo ligero (--no-tmp-container): NO crea contenedor temporal ── */
    if opts.no_tmp_container {
        if opts.dump.is_some() {
            return Err(CoolifyError::Validation(
                "--no-tmp-container no puede combinarse con --dump: para comparar contra un \
                 dump hay que restaurarlo (quita --no-tmp-container)".into(),
            ));
        }
        return execute_light(&ssh, &live, &live_schema, &settings, opts).await;
    }

    /* ── Modo completo: posible contenedor temporal, SIEMPRE limpiado ── */
    /* Barrido de recuperación: contenedores/dumps huérfanos de ejecuciones abortadas */
    let _ = db_tmp::cleanup_all_temp(&ssh).await;
    let mut tmp_guard: Option<db_tmp::TempDb> = None;
    /* Dump subido al VPS (si era local) — también se borra SIEMPRE */
    let mut remote_dump_guard: Option<String> = None;

    /* Bloque async para garantizar limpieza en todas las rutas (éxito o error) */
    let result = async {
        /* Determinar el objetivo */
        let (dump_path, contra, otro_creds, otro_schema, dump_restaurado, modo) =
            if let Some(other) = &opts.against {
                let target2 = settings.resolve_site_target(settings.get_site(other)?)?;
                let mut ssh2 = SshClient::from_vps(&target2.vps);
                ssh2.connect().await?;
                let o = resolve_live_creds(
                    &ssh2,
                    settings.get_site(other)?.stack_uuid.as_deref().unwrap_or_default(),
                )
                .await?;
                let os = discover_schema(&ssh2, &o).await?;
                (
                    None,
                    Some(other.clone()),
                    Some(o),
                    Some(os),
                    false,
                    "contra-sitio".to_string(),
                )
            } else {
                /* Dump: explícito o último VPS */
                let (dump, restored, modo) = if let Some(d) = &opts.dump {
                    (d.clone(), true, "completo".to_string())
                } else {
                    let d = find_latest_vps_dump(&ssh, stack_uuid).await?;
                    (d, true, "completo".to_string())
                };
                (Some(dump), None, None, None, restored, modo)
            };

        /* Si hay dump, restaurar en contenedor temporal y usar como "otro" */
        let mut otro_creds_local: Option<SideCreds> = None;
        let mut otro_schema_local: Option<SchemaModel> = None;
        let mut restored = dump_restaurado;

        if let Some(dump) = &dump_path {
            let image = detect_image_for(&live);
            let pw: SecretString = live
                .db_password
                .clone()
                .unwrap_or_else(|| SecretString::from("compare_tmp_pw"));
            let tmp = db_tmp::create_temp_container(
                &ssh, live.engine, &image, &live.db_user, &live.db_name, pw.expose_secret(),
            )
            .await?;
            tmp_guard = Some(tmp);
            let tmp_ref = tmp_guard.as_ref().ok_or_else(|| {
                CoolifyError::Internal("estado temporal de comparación no inicializado".to_string())
            })?;

            /* Si el dump es local, subirlo al VPS primero */
            let remote_dump = if Path::new(dump).exists() {
                let remote =
                    format!("/tmp/dbcompare_{}_{}.sql", opts.site_name, std::process::id());
                ssh.upload_file(Path::new(dump), &remote).await?;
                /* Registrar para borrarlo SIEMPRE en la limpieza final */
                remote_dump_guard = Some(remote.clone());
                remote
            } else {
                dump.clone()
            };

            db_tmp::restore_dump(&ssh, live.engine, tmp_ref, &remote_dump, pw.expose_secret())
                .await?;

            let o = SideCreds {
                engine: live.engine,
                container: tmp_ref.container.clone(),
                db_user: tmp_ref.db_user.clone(),
                db_name: tmp_ref.db_name.clone(),
                db_password: Some(pw.clone()),
            };
            let os = discover_schema(&ssh, &o).await?;
            otro_creds_local = Some(o);
            otro_schema_local = Some(os);
            restored = true;
        }

        /* Filtrado de tablas en ambos esquemas */
        let mut live_schema_mut = live_schema.clone();
        if let Some(f) = &table_filter {
            live_schema_mut.tables.retain(|k, _| f.contains(k));
        }
        if let Some(os) = &mut otro_schema_local {
            if let Some(f) = &table_filter {
                os.tables.retain(|k, _| f.contains(k));
            }
        }

        let otro_creds = otro_creds.as_ref().or(otro_creds_local.as_ref());
        let otro_schema = otro_schema.as_ref().or(otro_schema_local.as_ref());

        /* Comparar tablas presentes en ambos esquemas */
        let mut diffs: Vec<TableDiff> = Vec::new();
        let mut solo_vivo: Vec<String> = Vec::new();
        let mut solo_otro: Vec<String> = Vec::new();

        let (oc, os) = match (otro_creds, otro_schema) {
            (Some(oc), Some(os)) => (oc, os),
            _ => {
                /* Sin otro lado: todo lo vivo es "solo en vivo" */
                solo_vivo = live_schema_mut.tables.keys().cloned().collect();
                return Ok(CompareReport::build(
                    opts.site_name.clone(),
                    live.engine,
                    dump_path,
                    contra,
                    restored,
                    modo,
                    &diffs,
                    &solo_vivo,
                    &solo_otro,
                ));
            }
        };

        for (table, _info) in &live_schema_mut.tables {
            if let Some(other_info) = os.tables.get(table) {
                let diff = compare_table(
                    &ssh,
                    live.engine,
                    &live.container,
                    &live.db_user,
                    &live.db_name,
                    live.db_password.as_ref(),
                    &oc.container,
                    &oc.db_user,
                    &oc.db_name,
                    oc.db_password.as_ref(),
                    table,
                    other_info,
                    opts.extract_limit,
                    opts.limit_diff,
                )
                .await?;
                diffs.push(diff);
            } else {
                solo_vivo.push(table.clone());
            }
        }

        /* Tablas solo en el otro lado */
        for (table, _) in &os.tables {
            if !live_schema_mut.tables.contains_key(table) {
                solo_otro.push(table.clone());
            }
        }

        Ok(CompareReport::build(
            opts.site_name.clone(),
            live.engine,
            dump_path,
            contra,
            restored,
            modo,
            &diffs,
            &solo_vivo,
            &solo_otro,
        ))
    }
    .await;

    /* Limpieza SIEMPRE (éxito o error) */
    if let Some(tmp) = &tmp_guard {
        db_tmp::cleanup_temp(&ssh, &tmp.container).await;
    }
    /* Borrar también el dump temporal subido (no dejar basura en el VPS) */
    if let Some(remote) = &remote_dump_guard {
        let _ = ssh.execute(&format!("rm -f {remote}")).await;
    }

    result
}

/// Modo ligero: conteos + hash sin contenedor temporal.
/// Con `against`: compara digests vivos de dos sitios.
/// Sin `against`: solo digests del sitio vivo (sin referencia).
async fn execute_light(
    ssh: &SshClient,
    live: &SideCreds,
    live_schema: &SchemaModel,
    settings: &Settings,
    opts: &CompareOptions,
) -> std::result::Result<CompareReport, CoolifyError> {
    let table_filter: Option<Vec<String>> = opts.tables.as_ref().map(|t| {
        t.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    });

    /* Contra otro sitio vivo: comparar digests de ambos lados */
    if let Some(other) = &opts.against {
        let site2 = settings.get_site(other)?;
        validation::assert_site_ready(site2)?;
        let target2 = settings.resolve_site_target(site2)?;
        let mut ssh2 = SshClient::from_vps(&target2.vps);
        ssh2.connect().await?;
        let otro = resolve_live_creds(&ssh2, site2.stack_uuid.as_deref().unwrap_or_default()).await?;
        let mut otro_schema = discover_schema(&ssh2, &otro).await?;
        if let Some(f) = &table_filter {
            otro_schema.tables.retain(|k, _| f.contains(k));
        }

        let vivo_digest = digest_all(
            &ssh, live_schema, &live.container, &live.db_user, &live.db_name,
            live.db_password.as_ref(),
        )
        .await?;
        let otro_digest = digest_all(
            &ssh2, &otro_schema, &otro.container, &otro.db_user, &otro.db_name,
            otro.db_password.as_ref(),
        )
        .await?;

        let mut diffs: Vec<TableDiff> = Vec::new();
        let mut solo_vivo: Vec<String> = Vec::new();
        let mut solo_otro: Vec<String> = Vec::new();

        for (t, info) in &live_schema.tables {
            let d1 = &vivo_digest[t];
            match otro_digest.get(t) {
                Some(d2) => {
                    let comparable = !info.comparable_columns().is_empty();
                    let igual = comparable
                        && d1.hash.is_some()
                        && d2.hash.is_some()
                        && d1.hash == d2.hash;
                    diffs.push(TableDiff {
                        table: t.clone(),
                        rows_vivo: d1.row_count,
                        rows_otro: d2.row_count,
                        solo_en_vivo: Vec::new(),
                        solo_en_otro: Vec::new(),
                        diffs: if igual { 0 } else { 1 },
                        not_comparable: !comparable,
                        vector_ignored: info.has_vector(),
                    });
                }
                None => solo_vivo.push(t.clone()),
            }
        }
        for (t, _) in &otro_schema.tables {
            if !live_schema.tables.contains_key(t) {
                solo_otro.push(t.clone());
            }
        }

        return Ok(CompareReport::build(
            opts.site_name.clone(),
            live.engine,
            None,
            Some(other.clone()),
            false,
            "ligero-vivo".to_string(),
            &diffs,
            &solo_vivo,
            &solo_otro,
        ));
    }

    /* Sin referencia: digests del sitio vivo solos */
    let vivo_digest = digest_all(
        &ssh, live_schema, &live.container, &live.db_user, &live.db_name,
        live.db_password.as_ref(),
    )
    .await?;
    let mut diffs: Vec<TableDiff> = Vec::new();
    for (t, info) in &live_schema.tables {
        let d = &vivo_digest[t];
        diffs.push(TableDiff {
            table: t.clone(),
            rows_vivo: d.row_count,
            rows_otro: -1,
            solo_en_vivo: Vec::new(),
            solo_en_otro: Vec::new(),
            diffs: 0,
            not_comparable: d.not_comparable_light,
            vector_ignored: info.has_vector(),
        });
    }
    Ok(CompareReport::build(
        opts.site_name.clone(),
        live.engine,
        None,
        None,
        false,
        "ligero".to_string(),
        &diffs,
        &[],
        &[],
    ))
}

/// Busca el último dump VPS disponible para un stack.
/// Nota: los stacks PostgreSQL se guardan en `/data/backups/{uuid}` y los
/// MariaDB/WordPress en `/data/backups/mariadb-{uuid}` (script backup-server.sh).
async fn find_latest_vps_dump(
    ssh: &SshClient,
    stack_uuid: &str,
) -> std::result::Result<String, CoolifyError> {
    let base = format!("/data/backups/{stack_uuid}");
    let base_maria = format!("/data/backups/mariadb-{stack_uuid}");
    let cmd = format!(
        "ls -1t {base}/daily/*.sql.gz {base}/weekly/*.sql.gz \
         {base_maria}/daily/*.sql.gz {base_maria}/weekly/*.sql.gz 2>/dev/null | head -1"
    );
    let res = ssh.execute(&cmd).await?;
    let path = res.stdout.trim().to_string();
    if path.is_empty() {
        return Err(CoolifyError::Validation(format!(
            "No hay dump VPS para stack {stack_uuid}. Ejecuta 'backup' primero."
        )));
    }
    Ok(path)
}

/// Detecta la imagen Docker del motor para el contenedor temporal.
fn detect_image_for(creds: &SideCreds) -> String {
    match creds.engine {
        DbEngine::Postgres => "postgres:16-alpine".to_string(),
        DbEngine::MariaDb => "mariadb:11".to_string(),
    }
}

/// Helper para testing (usa credenciales reales solo en tests de integración).
pub fn _build_light_report(
    site_name: &str,
    engine: DbEngine,
    dump: Option<String>,
    live_digest: &BTreeMap<String, TableDigest>,
) -> CompareReport {
    let mut diffs = Vec::new();
    let mut solo_vivo = Vec::new();
    for (t, d) in live_digest {
        if d.not_comparable_light {
            diffs.push(TableDiff {
                table: t.clone(),
                rows_vivo: d.row_count,
                rows_otro: -1,
                solo_en_vivo: vec![],
                solo_en_otro: vec![],
                diffs: -1,
                not_comparable: true,
                vector_ignored: false,
            });
        } else {
            solo_vivo.push(t.clone());
        }
    }
    CompareReport::build(
        site_name.to_string(),
        engine,
        dump,
        None,
        false,
        "ligero".to_string(),
        &diffs,
        &solo_vivo,
        &[],
    )
}
