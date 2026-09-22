/* target_bootstrap_manager — install/uninstall de Coolify + bootstrap light.
 * Split 119A-5: tipos.rs + sondas.rs + coolify.rs + ligero.rs;
 * re-exports sin cambios para no tocar callers externos. */

mod coolify;
mod ligero;
mod sondas;
mod tipos;

pub use coolify::{install_coolify, uninstall_coolify};
pub use ligero::bootstrap_target_light;
pub use tipos::{BootstrapLightTargetReport, InstallCoolifyReport, UninstallCoolifyReport};
