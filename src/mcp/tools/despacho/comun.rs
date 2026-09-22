/*
 * MCP Tools — despacho/comun: helpers para extraer valores de args JSON.
 * (split 119A-5 de despacho.rs para bajar del limite de 500 lineas.)
 */

use crate::error::CoolifyError;
use serde_json::Value;

/// [119A-3] Extracción opcional de string (evita repetir el trío
/// `get().and_then(as_str).map(to_string)` en cada dispatcher).
pub(crate) fn get_opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

pub(crate) fn get_str(args: &Value, key: &str) -> std::result::Result<String, CoolifyError> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| CoolifyError::Validation(format!("Parametro requerido: '{key}'")))
}

pub(crate) fn get_str_or(args: &Value, key: &str, default: &str) -> String {
    args.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(default)
        .to_string()
}

pub(crate) fn get_bool(args: &Value, key: &str) -> bool {
    args.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}
