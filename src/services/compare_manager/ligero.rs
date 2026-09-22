/* Split 119A-5 de compare_manager.rs — modo ligero (conteos+hash, sin contenedor).
 * Código verbatim del original; solo cambia visibilidad de ayudantes compartidos. */

use super::origen::{discover_schema, resolve_live_creds};
use super::tipos::{CompareOptions, SideCreds};
use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::compare::diff::TableDiff;
use crate::services::compare::digest::{digest_all, TableDigest};
use crate::services::compare::report::CompareReport;
use crate::services::compare::schema::SchemaModel;

use std::collections::BTreeMap;

/// Modo ligero: conteos + hash sin contenedor temporal.
/// Con `against`: compara digests vivos de dos sitios.
/// Sin `against`: solo digests del sitio vivo (sin referencia).
pub(super) async fn execute_light(
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
        return comparar_ligero_contra(ssh, live, live_schema, settings, opts, other, &table_filter)
            .await;
    }

    /* Sin referencia: digests del sitio vivo solos */
    reporte_ligero_solo(ssh, live, live_schema, opts).await
}

/* Conecta al otro sitio, filtra su esquema y calcula digests de ambos lados. */
async fn conectar_otro_sitio(
    settings: &Settings,
    other: &str,
    table_filter: &Option<Vec<String>>,
) -> std::result::Result<(SshClient, SideCreds, SchemaModel), CoolifyError> {
    let site2 = settings.get_site(other)?;
    validation::assert_site_ready(site2)?;
    let target2 = settings.resolve_site_target(site2)?;
    let mut ssh2 = SshClient::from_vps(&target2.vps);
    ssh2.connect().await?;
    let otro =
        resolve_live_creds(&ssh2, site2.stack_uuid.as_deref().unwrap_or_default()).await?;
    let mut otro_schema = discover_schema(&ssh2, &otro).await?;
    if let Some(f) = table_filter {
        otro_schema.tables.retain(|k, _| f.contains(k));
    }
    Ok((ssh2, otro, otro_schema))
}

/* Compara digests vivos de dos sitios y construye el reporte ligero. */
async fn comparar_ligero_contra(
    ssh: &SshClient,
    live: &SideCreds,
    live_schema: &SchemaModel,
    settings: &Settings,
    opts: &CompareOptions,
    other: &str,
    table_filter: &Option<Vec<String>>,
) -> std::result::Result<CompareReport, CoolifyError> {
    let (ssh2, otro, otro_schema) = conectar_otro_sitio(settings, other, table_filter).await?;

    let vivo_digest = digest_all(
        ssh,
        live_schema,
        &live.container,
        &live.db_user,
        &live.db_name,
        live.db_password.as_ref(),
    )
    .await?;
    let otro_digest = digest_all(
        &ssh2,
        &otro_schema,
        &otro.container,
        &otro.db_user,
        &otro.db_name,
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
                let igual =
                    comparable && d1.hash.is_some() && d2.hash.is_some() && d1.hash == d2.hash;
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
    for t in otro_schema.tables.keys() {
        if !live_schema.tables.contains_key(t) {
            solo_otro.push(t.clone());
        }
    }

    Ok(CompareReport::build(
        opts.site_name.clone(),
        live.engine,
        None,
        Some(other.to_string()),
        false,
        "ligero-vivo".to_string(),
        true,
        &diffs,
        &solo_vivo,
        &solo_otro,
    ))
}

/* Sin referencia: digests del sitio vivo solos. */
async fn reporte_ligero_solo(
    ssh: &SshClient,
    live: &SideCreds,
    live_schema: &SchemaModel,
    opts: &CompareOptions,
) -> std::result::Result<CompareReport, CoolifyError> {
    /* Sin referencia: digests del sitio vivo solos */
    let vivo_digest = digest_all(
        ssh,
        live_schema,
        &live.container,
        &live.db_user,
        &live.db_name,
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
        false,
        &diffs,
        &[],
        &[],
    ))
}

/// Helper para testing (usa credenciales reales solo en tests de integración).
pub fn _build_light_report(
    site_name: &str,
    engine: crate::services::compare::schema::DbEngine,
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
        false,
        &diffs,
        &solo_vivo,
        &[],
    )
}
