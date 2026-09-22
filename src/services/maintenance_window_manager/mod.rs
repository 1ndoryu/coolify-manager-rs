/* maintenance_window_manager — ventana de mantenimiento (evaluar + programar).
 * Split 119A-5: tipos.rs + sondas.rs + deriva.rs + evaluacion.rs + programacion.rs;
 * re-exports sin cambios para no tocar callers externos. */

mod deriva;
mod evaluacion;
mod programacion;
mod sondas;
mod tipos;

pub use evaluacion::{evaluate_default_vps, evaluate_target};
pub use programacion::schedule_target;
pub use tipos::{
    MaintenanceSiteHealth, MaintenanceWindowReport, MaintenanceWindowRequest,
    ScheduleMaintenanceReport, ScheduleMaintenanceRequest,
};
