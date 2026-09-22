/*
 * DNS manager — tipos: reportes de switch/borrado y vistas internas.
 * (split 119A-5 de dns_manager.rs para bajar del god-object de 507 lineas;
 * codigo verbatim del original.)
 */

use crate::domain::SiteDnsRecord;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DnsSwitchAction {
    pub record_name: String,
    pub record_type: String,
    pub action: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DnsSwitchReport {
    pub provider: String,
    pub zone: String,
    pub target_ip: String,
    pub dry_run: bool,
    pub actions: Vec<DnsSwitchAction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DnsDeleteAction {
    pub record_name: String,
    pub record_type: String,
    pub action: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DnsDeleteReport {
    pub provider: String,
    pub zone: String,
    pub vps_ip: String,
    pub dry_run: bool,
    pub actions: Vec<DnsDeleteAction>,
}

/* Vista normalizada de un registro existente (común a ambos proveedores). */
#[derive(Debug, Clone)]
pub(crate) struct RegistroExistente {
    pub nombre: String,
    pub tipo: String,
    pub contenido: String,
    pub id: String,
}

/* Decisión pura por registro deseado: acción de reporte + id a borrar. */
#[derive(Debug, Clone)]
pub(crate) struct DecisionBorrado {
    pub action: DnsDeleteAction,
    pub id_a_borrar: Option<String>,
}

/* Entrada resuelta para conciliar: registros deseados + destino. */
pub(crate) struct ObjetivoSwitch {
    pub zone: String,
    pub target_ip: String,
    pub dry_run: bool,
    pub desired: Vec<SiteDnsRecord>,
}
