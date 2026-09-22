/* host_optimization_manager — optimización host-level (swap, sysctl, THP, live-restore).
 * Split 119A-5: tipos.rs + diagnostico.rs + aplicacion.rs + entrada.rs;
 * re-exports sin cambios para no tocar callers externos. */

mod aplicacion;
mod diagnostico;
mod entrada;
mod tipos;

pub use entrada::{optimize_default_vps, optimize_target, optimize_vps_config};
pub use tipos::{HostOptimizationReport, HostOptimizationRequest};
