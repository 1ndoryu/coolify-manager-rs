/*
 * compare/report — modelo de salida JSON estable para db-compare.
 * E12: el reporte es la evidencia reproducible de la comparación.
 * Nunca contiene credenciales ni datos sensibles sin redactar.
 */

use crate::error::CoolifyError;
use crate::services::compare::diff::TableDiff;
use crate::services::compare::schema::DbEngine;

use serde::Serialize;

/// Estado de una tabla en la comparación.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TableState {
    Identica,
    ConDiferencia,
    SoloEnVivo,
    SoloEnOtro,
    NoComparable,
    /// Sin referencia: modo ligero sin baseline (rows_otro == -1).
    /// Nunca debe leerse como "idéntica": no certifica no-pérdida.
    SinReferencia,
}

/// Veredicto global v2: un reporte solo certifica no-pérdida en VERDE.
/// AMARILLO informa diferencias; GRIS no certifica (sin baseline fijada);
/// ROJO exige actuar (posible vaciado o negocio en cero).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Veredicto {
    Verde,
    Amarillo,
    Rojo,
    Gris,
}

/// Entrada por tabla del reporte.
#[derive(Debug, Clone, Serialize)]
pub struct TableReport {
    pub table: String,
    pub state: TableState,
    pub rows_vivo: i64,
    pub rows_otro: i64,
    pub diffs: i64,
    pub solo_en_vivo: Vec<String>,
    pub solo_en_otro: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_ignored: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_comparable: Option<bool>,
}

/// Resumen agregado.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Summary {
    pub tables_vivo: usize,
    pub tables_otro: usize,
    pub tables_solo_vivo: usize,
    pub tables_solo_otro: usize,
    pub tables_identicas: usize,
    pub tables_con_diferencia: usize,
    pub tables_no_comparables: usize,
    pub tables_sin_referencia: usize,
}

/// Reporte completo JSON.
#[derive(Debug, Clone, Serialize)]
pub struct CompareReport {
    pub sitio: String,
    pub motor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dump: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contra: Option<String>,
    pub dump_restaurado: bool,
    pub modo: String,
    pub fecha_verificacion: String,
    /// Fecha extraída del nombre del dump (None si no se pudo determinar).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fecha_dump: Option<String>,
    /// true solo si --dump/--against fue fijado explícitamente.
    /// El modo "último dump" informa pero nunca certifica (veredicto GRIS).
    pub baseline_fijada: bool,
    pub veredicto: Veredicto,
    pub detalle_veredicto: String,
    pub resumen: Summary,
    pub tablas: Vec<TableReport>,
}

/// Clasifica una tabla según su diff.
/// `rows_otro == -1` significa "sin referencia" (modo ligero sin baseline):
/// se clasifica SinReferencia y nunca Identica.
pub fn classify(diff: &TableDiff) -> TableState {
    if diff.rows_otro == -1 {
        return TableState::SinReferencia;
    }
    if diff.not_comparable {
        return TableState::NoComparable;
    }
    if diff.rows_vivo > 0 && diff.rows_otro == 0 {
        return TableState::SoloEnVivo;
    }
    if diff.rows_otro > 0 && diff.rows_vivo == 0 {
        return TableState::SoloEnOtro;
    }
    if diff.diffs == 0 {
        TableState::Identica
    } else {
        TableState::ConDiferencia
    }
}

/// Tabla crítica de negocio por motor: si existe en vivo con 0 filas,
/// el sitio está roto aunque el resto coincida (detector negocio-en-cero).
fn es_tabla_critica(motor: &str, tabla: &str) -> bool {
    let t = tabla.to_lowercase();
    if motor == "mariadb" {
        /* WordPress: cualquier prefijo (wp_users, xyz_posts, ...) */
        t.ends_with("_users") || t.ends_with("_posts") || t == "users" || t == "posts"
    } else {
        /* Stacks Rust/Postgres: nombres exactos */
        t == "users" || t == "projects" || t == "orders"
    }
}

/// Extrae fecha YYYY-MM-DD del nombre de un dump.
/// Soporta `20260813_030007` (legacy) y `2026-09-09_0100` (rotativo VPS).
/// Devuelve None si el nombre no contiene fecha reconocible.
pub fn fecha_desde_nombre_dump(ruta: &str) -> Option<String> {
    let nombre = ruta.rsplit('/').next().unwrap_or(ruta);
    /* Formato con guiones: 2026-09-09 */
    if let Some(pos) = nombre.find(|c: char| c.is_ascii_digit()) {
        let resto = &nombre[pos..];
        if resto.len() >= 10 {
            let c: Vec<char> = resto.chars().collect();
            if c[4] == '-'
                && c[7] == '-'
                && c[0..4].iter().all(|x| x.is_ascii_digit())
                && c[5..7].iter().all(|x| x.is_ascii_digit())
                && c[8..10].iter().all(|x| x.is_ascii_digit())
            {
                let (y, m, d) = (
                    resto[0..4].parse::<u32>().unwrap_or(0),
                    resto[5..7].parse::<u32>().unwrap_or(0),
                    resto[8..10].parse::<u32>().unwrap_or(0),
                );
                if (1990..=2100).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d) {
                    return Some(resto[0..10].to_string());
                }
            }
        }
        /* Formato compacto: 20260813 */
        let digitos: String = resto.chars().take(8).collect();
        if digitos.len() == 8 && digitos.chars().all(|x| x.is_ascii_digit()) {
            let (y, m, d) = (
                digitos[0..4].parse::<u32>().unwrap_or(0),
                digitos[4..6].parse::<u32>().unwrap_or(0),
                digitos[6..8].parse::<u32>().unwrap_or(0),
            );
            if (1990..=2100).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d) {
                return Some(format!(
                    "{}-{}-{}",
                    &digitos[0..4],
                    &digitos[4..6],
                    &digitos[6..8]
                ));
            }
        }
    }
    None
}

/// Calcula el veredicto global v2 (función pura, testeable).
/// Orden: ambos-vacíos → negocio-en-cero → sin-baseline → diferencias → verde.
pub fn veredicto(
    motor: &str,
    baseline_fijada: bool,
    diffs: &[TableDiff],
    solo_vivo: usize,
    solo_otro: usize,
    con_diferencia: usize,
    no_comparables: usize,
) -> (Veredicto, String) {
    let total_vivo = diffs.len() + solo_vivo;
    let total_otro = diffs.len() + solo_otro;
    /* 1. Ambos lados sin tablas: posible vaciado doble, nunca "idénticas" */
    if total_vivo == 0 && total_otro == 0 {
        return (
            Veredicto::Rojo,
            "ambos-vacíos: viva y referencia sin tablas; posible vaciado".into(),
        );
    }
    /* 2. Negocio en cero: tabla crítica viva con 0 filas (define roto siempre) */
    for d in diffs {
        if d.rows_vivo == 0 && es_tabla_critica(motor, &d.table) {
            return (
                Veredicto::Rojo,
                format!(
                    "negocio-en-cero: tabla crítica '{}' vacía en vivo (otro={})",
                    d.table, d.rows_otro
                ),
            );
        }
    }
    /* 3. Sin baseline fijada: el reporte informa, no certifica */
    if !baseline_fijada {
        return (
            Veredicto::Gris,
            "sin-baseline fijada: conteos solo-viva; fija --dump o --against para certificar"
                .into(),
        );
    }
    /* 4. Diferencias reales */
    if con_diferencia > 0 || solo_vivo > 0 || solo_otro > 0 {
        return (
            Veredicto::Amarillo,
            format!(
                "diverge: {con_diferencia} con-diferencia, {solo_vivo} solo-vivo, {solo_otro} solo-otro"
            ),
        );
    }
    if no_comparables > 0 {
        return (
            Veredicto::Amarillo,
            format!("sin diferencias comparables pero {no_comparables} tablas no comparables"),
        );
    }
    (Veredicto::Verde, "idéntica contra baseline fijada".into())
}

impl CompareReport {
    /// Construye el reporte desde los diffs por tabla.
    /// `baseline_fijada` debe ser true solo con --dump/--against explícito.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        sitio: String,
        engine: DbEngine,
        dump: Option<String>,
        contra: Option<String>,
        dump_restaurado: bool,
        modo: String,
        baseline_fijada: bool,
        diffs: &[TableDiff],
        solo_vivo_tables: &[String],
        solo_otro_tables: &[String],
    ) -> Self {
        let mut tables = Vec::new();
        let mut summary = Summary::default();
        summary.tables_vivo = diffs.len() + solo_vivo_tables.len();
        summary.tables_otro = diffs.len() + solo_otro_tables.len();
        summary.tables_solo_vivo = solo_vivo_tables.len();
        summary.tables_solo_otro = solo_otro_tables.len();

        for d in diffs {
            let state = classify(d);
            match state {
                TableState::Identica => summary.tables_identicas += 1,
                TableState::ConDiferencia => summary.tables_con_diferencia += 1,
                TableState::NoComparable => summary.tables_no_comparables += 1,
                TableState::SinReferencia => summary.tables_sin_referencia += 1,
                _ => {}
            }
            tables.push(TableReport {
                table: d.table.clone(),
                state,
                rows_vivo: d.rows_vivo,
                rows_otro: d.rows_otro,
                diffs: d.diffs,
                solo_en_vivo: d.solo_en_vivo.clone(),
                solo_en_otro: d.solo_en_otro.clone(),
                vector_ignored: if d.vector_ignored { Some(true) } else { None },
                not_comparable: if d.not_comparable { Some(true) } else { None },
            });
        }

        for t in solo_vivo_tables {
            tables.push(TableReport {
                table: t.clone(),
                state: TableState::SoloEnVivo,
                rows_vivo: -1,
                rows_otro: 0,
                diffs: -1,
                solo_en_vivo: Vec::new(),
                solo_en_otro: Vec::new(),
                vector_ignored: None,
                not_comparable: None,
            });
        }
        for t in solo_otro_tables {
            tables.push(TableReport {
                table: t.clone(),
                state: TableState::SoloEnOtro,
                rows_vivo: 0,
                rows_otro: -1,
                diffs: -1,
                solo_en_vivo: Vec::new(),
                solo_en_otro: Vec::new(),
                vector_ignored: None,
                not_comparable: None,
            });
        }

        let motor = engine.as_str().to_string();
        let (veredicto, detalle_veredicto) = veredicto(
            &motor,
            baseline_fijada,
            diffs,
            solo_vivo_tables.len(),
            solo_otro_tables.len(),
            summary.tables_con_diferencia,
            summary.tables_no_comparables,
        );
        let fecha_dump = dump.as_deref().and_then(fecha_desde_nombre_dump);

        CompareReport {
            sitio,
            motor,
            dump,
            contra,
            dump_restaurado,
            modo,
            fecha_verificacion: chrono::Utc::now().to_rfc3339(),
            fecha_dump,
            baseline_fijada,
            veredicto,
            detalle_veredicto,
            resumen: summary,
            tablas: tables,
        }
    }

    /// Serializa a JSON (pretty cuando human=false usamos compacto; aquí stable).
    pub fn to_json(&self) -> std::result::Result<String, CoolifyError> {
        serde_json::to_string_pretty(self).map_err(|e| {
            CoolifyError::Validation(format!("Error serializando reporte: {e}"))
        })
    }

    /// Renderiza texto formateado legible.
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "=== db-compare: {} ({} ===\n",
            self.sitio, self.motor
        ));
        if let Some(d) = &self.dump {
            s.push_str(&format!("Dump: {}\n", d));
        }
        if let Some(c) = &self.contra {
            s.push_str(&format!("Contra sitio: {}\n", c));
        }
        s.push_str(&format!(
            "Modo: {} | dump_restaurado: {} | baseline_fijada: {}\n",
            self.modo, self.dump_restaurado, self.baseline_fijada
        ));
        s.push_str(&format!(
            "Veredicto: {:?} — {}\n",
            self.veredicto, self.detalle_veredicto
        ));
        if let Some(f) = &self.fecha_dump {
            s.push_str(&format!(
                "Fecha dump: {f} | verificación: {}\n",
                self.fecha_verificacion
            ));
        }
        s.push_str(&format!(
            "Resumen: {} idénticas, {} con diferencia, {} solo-en-vivo, {} solo-en-otro, {} no comparables, {} sin referencia\n",
            self.resumen.tables_identicas,
            self.resumen.tables_con_diferencia,
            self.resumen.tables_solo_vivo,
            self.resumen.tables_solo_otro,
            self.resumen.tables_no_comparables,
            self.resumen.tables_sin_referencia
        ));

        for t in &self.tablas {
            s.push_str(&format!(
                "  - {}: {:?} (vivo={}, otro={}, diffs={})",
                t.table, t.state, t.rows_vivo, t.rows_otro, t.diffs
            ));
            if let Some(v) = t.vector_ignored {
                if v {
                    s.push_str(" [vector_ignored]");
                }
            }
            if !t.solo_en_vivo.is_empty() {
                s.push_str(&format!(
                    "\n      solo_en_vivo ({}): {:?}",
                    t.solo_en_vivo.len(),
                    t.solo_en_vivo
                ));
            }
            if !t.solo_en_otro.is_empty() {
                s.push_str(&format!(
                    "\n      solo_en_otro ({}): {:?}",
                    t.solo_en_otro.len(),
                    t.solo_en_otro
                ));
            }
            s.push('\n');
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_diff(table: &str, rv: i64, ro: i64, diffs: i64, nc: bool) -> TableDiff {
        TableDiff {
            table: table.into(),
            rows_vivo: rv,
            rows_otro: ro,
            solo_en_vivo: vec![],
            solo_en_otro: vec![],
            diffs,
            not_comparable: nc,
            vector_ignored: false,
        }
    }

    #[test]
    fn test_classify() {
        assert_eq!(
            classify(&mk_diff("a", 10, 10, 0, false)),
            TableState::Identica
        );
        assert_eq!(
            classify(&mk_diff("a", 10, 10, 2, false)),
            TableState::ConDiferencia
        );
        assert_eq!(
            classify(&mk_diff("a", 5, 0, 0, false)),
            TableState::SoloEnVivo
        );
        assert_eq!(
            classify(&mk_diff("a", 0, 5, 0, false)),
            TableState::SoloEnOtro
        );
        assert_eq!(
            classify(&mk_diff("a", 5, 5, 1, true)),
            TableState::NoComparable
        );
    }

    #[test]
    fn test_report_json_serializa() {
        let r = CompareReport::build(
            "studio".into(),
            DbEngine::Postgres,
            Some("/data/backups/x.sql.gz".into()),
            None,
            true,
            "completo".into(),
            true,
            &[mk_diff("t1", 10, 10, 0, false)],
            &["solo_vivo".into()],
            &[],
        );
        let json = r.to_json().unwrap();
        assert!(json.contains("\"sitio\": \"studio\""));
        assert!(json.contains("\"tables_identicas\": 1"));
    }

    /* ── Tests v2 (Fase 3): veredicto que nunca da verde ante un vaciado ── */

    #[test]
    fn test_sin_referencia_no_es_identica() {
        /* Modo ligero sin baseline: rows_otro=-1 → SinReferencia, nunca Identica */
        assert_eq!(
            classify(&mk_diff("wp_posts", 41, -1, 0, false)),
            TableState::SinReferencia
        );
        let r = CompareReport::build(
            "guillermo".into(),
            DbEngine::MariaDb,
            None,
            None,
            false,
            "ligero".into(),
            false,
            &[mk_diff("wp_posts", 41, -1, 0, false)],
            &[],
            &[],
        );
        assert_eq!(r.veredicto, Veredicto::Gris);
        assert_eq!(r.resumen.tables_sin_referencia, 1);
        assert_eq!(r.resumen.tables_identicas, 0);
    }

    #[test]
    fn test_ambos_vacios_rojo() {
        let (v, _) = veredicto("mariadb", true, &[], 0, 0, 0, 0);
        assert_eq!(v, Veredicto::Rojo);
    }

    #[test]
    fn test_viva_vacia_vs_dump_con_datos_rojo() {
        /* Caso studio: wp_users vacía en vivo, con datos en el dump */
        let diffs = vec![
            mk_diff("wp_users", 0, 5, 0, false),
            mk_diff("wp_posts", 0, 41, 0, false),
        ];
        let (v, detalle) = veredicto("mariadb", true, &diffs, 0, 0, 0, 0);
        assert_eq!(v, Veredicto::Rojo);
        assert!(detalle.contains("negocio-en-cero"));
    }

    #[test]
    fn test_pin_baseline_obligatorio() {
        /* Idénticas pero con baseline auto-resuelta (no fijada) → GRIS, nunca VERDE */
        let diffs = vec![mk_diff("wp_posts", 41, 41, 0, false)];
        let (v, _) = veredicto("mariadb", false, &diffs, 0, 0, 0, 0);
        assert_eq!(v, Veredicto::Gris);
        let (v2, _) = veredicto("mariadb", true, &diffs, 0, 0, 0, 0);
        assert_eq!(v2, Veredicto::Verde);
    }

    #[test]
    fn test_fecha_dump_desde_nombre() {
        assert_eq!(
            fecha_desde_nombre_dump(
                "/data/backups/coolify-manager/guillermo/daily/20260813_030007.tar.gz"
            ),
            Some("2026-08-13".to_string())
        );
        assert_eq!(
            fecha_desde_nombre_dump("/data/backups/mariadb-xyz/daily/2026-09-09_0100.sql.gz"),
            Some("2026-09-09".to_string())
        );
        assert_eq!(fecha_desde_nombre_dump("/tmp/dbcompare_x.sql"), None);
    }
}
