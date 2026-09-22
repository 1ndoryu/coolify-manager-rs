/* Auditor del plano de control Coolify (119A-5 split: barrido, reparacion y stop/start/status). */

pub mod auditoria;
pub mod control;
pub mod ejecucion;
pub mod formato;
pub mod recomendaciones;
pub mod redis;
pub mod reparacion;
pub mod tipos;

pub use auditoria::{audit_default_vps, audit_target};
pub use control::{
    start_control_plane_target, status_control_plane_target, stop_control_plane_target,
};
pub use reparacion::{repair_default_vps, repair_target};
pub use tipos::ControlPlaneAuditReport;
