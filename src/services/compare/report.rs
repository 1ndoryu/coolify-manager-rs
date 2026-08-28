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
    pub resumen: Summary,
    pub tablas: Vec<TableReport>,
}

/// Clasifica una tabla según su diff.
pub fn classify(diff: &TableDiff) -> TableState {
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

impl CompareReport {
    /// Construye el reporte desde los diffs por tabla.
    pub fn build(
        sitio: String,
        engine: DbEngine,
        dump: Option<String>,
        contra: Option<String>,
        dump_restaurado: bool,
        modo: String,
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

        CompareReport {
            sitio,
            motor: engine.as_str().to_string(),
            dump,
            contra,
            dump_restaurado,
            modo,
            fecha_verificacion: chrono::Utc::now().to_rfc3339(),
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
        s.push_str(&format!("Modo: {} | dump_restaurado: {}\n", self.modo, self.dump_restaurado));
        s.push_str(&format!(
            "Resumen: {} idénticas, {} con diferencia, {} solo-en-vivo, {} solo-en-otro, {} no comparables\n",
            self.resumen.tables_identicas,
            self.resumen.tables_con_diferencia,
            self.resumen.tables_solo_vivo,
            self.resumen.tables_solo_otro,
            self.resumen.tables_no_comparables
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
                s.push_str(&format!("\n      solo_en_vivo ({}): {:?}", t.solo_en_vivo.len(), t.solo_en_vivo));
            }
            if !t.solo_en_otro.is_empty() {
                s.push_str(&format!("\n      solo_en_otro ({}): {:?}", t.solo_en_otro.len(), t.solo_en_otro));
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
        assert_eq!(classify(&mk_diff("a", 10, 10, 0, false)), TableState::Identica);
        assert_eq!(classify(&mk_diff("a", 10, 10, 2, false)), TableState::ConDiferencia);
        assert_eq!(classify(&mk_diff("a", 5, 0, 0, false)), TableState::SoloEnVivo);
        assert_eq!(classify(&mk_diff("a", 0, 5, 0, false)), TableState::SoloEnOtro);
        assert_eq!(classify(&mk_diff("a", 5, 5, 1, true)), TableState::NoComparable);
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
            &[mk_diff("t1", 10, 10, 0, false)],
            &["solo_vivo".into()],
            &[],
        );
        let json = r.to_json().unwrap();
        assert!(json.contains("\"sitio\": \"studio\""));
        assert!(json.contains("\"tables_identicas\": 1"));
    }
}
