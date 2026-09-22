/* Split 119A-5 de compare_manager.rs — modo completo (contenedor temporal efímero).
 * Código verbatim del original; solo cambia visibilidad de ayudantes compartidos. */

use super::dumps::{detect_image_for, find_latest_vps_dump, resolve_legacy_dump};
use super::ligero::execute_light;
use super::origen::{discover_schema, resolve_live_creds};
use super::tipos::{CompareOptions, SideCreds};
use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::db_tmp;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::compare::diff::{compare_table, TableDiff};
use crate::services::compare::report::CompareReport;
use crate::services::compare::schema::SchemaModel;

use secrecy::{ExposeSecret, SecretString};
use std::path::Path;

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
                 dump hay que restaurarlo (quita --no-tmp-container)"
                    .into(),
            ));
        }
        return execute_light(&ssh, &live, &live_schema, &settings, opts).await;
    }

    /* ── Modo completo: posible contenedor temporal, SIEMPRE limpiado ── */
    /* Barrido de recuperación: contenedores/dumps huérfanos de ejecuciones abortadas */
    let _ = db_tmp::cleanup_all_temp(&ssh).await;
    let mut tmp_guard: Option<db_tmp::TempDb> = None;
    /* Dumps temporales en el VPS (subidos o extraídos) — se borran SIEMPRE.
    NUNCA se registra aquí una ruta de backup real (VPS/legacy): solo /tmp propio. */
    let mut remote_dump_guard: Vec<String> = Vec::new();

    /* Bloque async para garantizar limpieza en todas las rutas (éxito o error) */
    let result = async {
        /* Determinar el objetivo */
        let mut objetivo = resolver_objetivo(&ssh, &settings, opts, stack_uuid).await?;

        /* Si hay dump, restaurar en contenedor temporal y usar como "otro" */
        if let Some(dump) = objetivo.dump_path.clone() {
            let (o, os) = restaurar_en_temporal(
                &ssh,
                &live,
                opts,
                &dump,
                &mut tmp_guard,
                &mut remote_dump_guard,
            )
            .await?;
            objetivo.otro_creds = Some(o);
            objetivo.otro_schema = Some(os);
            objetivo.dump_restaurado = true;
        }

        comparar_objetivo(&ssh, &live, &live_schema, &table_filter, objetivo, opts).await
    }
    .await;

    /* Limpieza SIEMPRE (éxito o error) */
    if let Some(tmp) = &tmp_guard {
        db_tmp::cleanup_temp(&ssh, &tmp.container).await;
    }
    /* Borrar también los dumps temporales subidos/extraídos (no dejar basura en el VPS).
    Solo contiene rutas /tmp propias registradas arriba; jamás backups reales. */
    for remote in &remote_dump_guard {
        if remote.starts_with("/tmp/dbcompare_") {
            let _ = ssh.execute(&format!("rm -f {remote}")).await;
        }
    }

    result
}

/* Objetivo de comparacion: dump o contra-sitio resuelto. */
struct ObjetivoComparacion {
    dump_path: Option<String>,
    contra: Option<String>,
    otro_creds: Option<SideCreds>,
    otro_schema: Option<SchemaModel>,
    dump_restaurado: bool,
    modo: String,
    baseline_fijada: bool,
}

/* Resuelve contra que se compara: otro sitio en vivo o dump (fijado/legacy/ultimo). */
async fn resolver_objetivo(
    ssh: &SshClient,
    settings: &Settings,
    opts: &CompareOptions,
    stack_uuid: &str,
) -> std::result::Result<ObjetivoComparacion, CoolifyError> {
    if let Some(other) = &opts.against {
        let target2 = settings.resolve_site_target(settings.get_site(other)?)?;
        let mut ssh2 = SshClient::from_vps(&target2.vps);
        ssh2.connect().await?;
        let o = resolve_live_creds(
            &ssh2,
            settings
                .get_site(other)?
                .stack_uuid
                .as_deref()
                .unwrap_or_default(),
        )
        .await?;
        let os = discover_schema(&ssh2, &o).await?;
        Ok(ObjetivoComparacion {
            dump_path: None,
            contra: Some(other.clone()),
            otro_creds: Some(o),
            otro_schema: Some(os),
            dump_restaurado: false,
            modo: "contra-sitio".to_string(),
            baseline_fijada: true,
        })
    } else {
        /* Dump: explícito (fijado) o último VPS (solo informa, no certifica) */
        let (dump, restored, modo, baseline) = if let Some(d) = &opts.dump {
            if let Some(sufijo) = d.strip_prefix("legacy:") {
                let r = resolve_legacy_dump(ssh, &opts.site_name, sufijo).await?;
                (r, true, "completo-legacy".to_string(), true)
            } else {
                (d.clone(), true, "completo".to_string(), true)
            }
        } else {
            let d = find_latest_vps_dump(ssh, stack_uuid).await?;
            (d, true, "completo-ultimo-dump".to_string(), false)
        };
        Ok(ObjetivoComparacion {
            dump_path: Some(dump),
            contra: None,
            otro_creds: None,
            otro_schema: None,
            dump_restaurado: restored,
            modo,
            baseline_fijada: baseline,
        })
    }
}

/* Restaura el dump en un contenedor temporal y descubre su esquema. */
async fn restaurar_en_temporal(
    ssh: &SshClient,
    live: &SideCreds,
    opts: &CompareOptions,
    dump: &str,
    tmp_guard: &mut Option<db_tmp::TempDb>,
    remote_dump_guard: &mut Vec<String>,
) -> std::result::Result<(SideCreds, SchemaModel), CoolifyError> {
    let image = detect_image_for(live);
    let pw: SecretString = live
        .db_password
        .clone()
        .unwrap_or_else(|| SecretString::from("compare_tmp_pw"));
    let tmp = db_tmp::create_temp_container(
        ssh,
        live.engine,
        &image,
        &live.db_user,
        &live.db_name,
        pw.expose_secret(),
    )
    .await?;
    *tmp_guard = Some(tmp);
    let tmp_ref = tmp_guard.as_ref().ok_or_else(|| {
        CoolifyError::Internal("estado temporal de comparación no inicializado".to_string())
    })?;

    /* Si el dump es local, subirlo al VPS primero */
    let remote_dump = if Path::new(dump).exists() {
        let remote = format!(
            "/tmp/dbcompare_{}_{}.sql",
            opts.site_name,
            std::process::id()
        );
        ssh.upload_file(Path::new(dump), &remote).await?;
        /* Registrar para borrarlo SIEMPRE en la limpieza final */
        remote_dump_guard.push(remote.clone());
        remote
    } else {
        dump.to_string()
    };

    /* Tarball legacy (.tar.gz): extraer el db-*.sql a /tmp propio
    (el tarball fuente solo se lee). El extraído también se borra SIEMPRE.
    Nota: se mira la extensión del dump ORIGINAL porque el subido
    local se renombra a /tmp/dbcompare_*.sql. */
    let mut remote_dump = remote_dump;
    let es_tarball = dump.ends_with(".tar.gz") || dump.ends_with(".tgz");
    if es_tarball {
        let safe_site: String = opts
            .site_name
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        let extracted = format!(
            "/tmp/dbcompare_{safe_site}_{}_legacy.sql",
            std::process::id()
        );
        let miembro = db_tmp::extract_sql_from_tarball(ssh, &remote_dump, &extracted).await?;
        let _ = miembro;
        remote_dump_guard.push(extracted.clone());
        remote_dump = extracted;
    }

    db_tmp::restore_dump(ssh, live.engine, tmp_ref, &remote_dump, pw.expose_secret()).await?;

    let o = SideCreds {
        engine: live.engine,
        container: tmp_ref.container.clone(),
        db_user: tmp_ref.db_user.clone(),
        db_name: tmp_ref.db_name.clone(),
        db_password: Some(pw.clone()),
    };
    let os = discover_schema(ssh, &o).await?;
    Ok((o, os))
}

/* Filtra esquemas, compara tablas en ambos lados y construye el reporte. */
async fn comparar_objetivo(
    ssh: &SshClient,
    live: &SideCreds,
    live_schema: &SchemaModel,
    table_filter: &Option<Vec<String>>,
    mut objetivo: ObjetivoComparacion,
    opts: &CompareOptions,
) -> std::result::Result<CompareReport, CoolifyError> {
    /* Filtrado de tablas en ambos esquemas */
    let mut live_schema_mut = live_schema.clone();
    if let Some(f) = table_filter {
        live_schema_mut.tables.retain(|k, _| f.contains(k));
    }
    if let Some(os) = &mut objetivo.otro_schema {
        if let Some(f) = table_filter {
            os.tables.retain(|k, _| f.contains(k));
        }
    }

    /* Comparar tablas presentes en ambos esquemas */
    let mut diffs: Vec<TableDiff> = Vec::new();
    let mut solo_vivo: Vec<String> = Vec::new();
    let mut solo_otro: Vec<String> = Vec::new();

    let (oc, os) = match (&objetivo.otro_creds, &objetivo.otro_schema) {
        (Some(oc), Some(os)) => (oc, os),
        _ => {
            /* Sin otro lado: todo lo vivo es "solo en vivo" */
            solo_vivo = live_schema_mut.tables.keys().cloned().collect();
            return Ok(CompareReport::build(
                opts.site_name.clone(),
                live.engine,
                objetivo.dump_path.clone(),
                objetivo.contra.clone(),
                objetivo.dump_restaurado,
                objetivo.modo.clone(),
                objetivo.baseline_fijada,
                &diffs,
                &solo_vivo,
                &solo_otro,
            ));
        }
    };

    for table in live_schema_mut.tables.keys() {
        if let Some(other_info) = os.tables.get(table) {
            let diff = compare_table(
                ssh,
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
    for table in os.tables.keys() {
        if !live_schema_mut.tables.contains_key(table) {
            solo_otro.push(table.clone());
        }
    }

    Ok(CompareReport::build(
        opts.site_name.clone(),
        live.engine,
        objetivo.dump_path,
        objetivo.contra,
        objetivo.dump_restaurado,
        objetivo.modo,
        objetivo.baseline_fijada,
        &diffs,
        &solo_vivo,
        &solo_otro,
    ))
}
