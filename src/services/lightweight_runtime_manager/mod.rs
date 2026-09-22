/* sentinel-disable-file limite-lineas: servicio central del runtime lightweight.
 * El archivo ya concentraba inventario/provisioning/control antes de 245A-9; este
 * bloque cierra backup/restore sin mezclar un refactor estructural de 1000+ lineas.
 * Split 119A-5: modulos en tipos.rs + plantillas.rs + sitios.rs + control.rs +
 * respaldo.rs; re-exports sin cambios para no tocar callers externos. */

mod control;
mod plantillas;
mod respaldo;
mod sitios;
mod tipos;

pub use control::control_lightweight_site;
pub use respaldo::{
    create_lightweight_site_backup, list_lightweight_site_backups, restore_lightweight_site_backup,
};
pub use sitios::{inventory_light_target, provision_static_site};
pub use tipos::{
    LightweightBackupEntry, LightweightBackupListReport, LightweightBackupReport,
    LightweightInventoryReport, LightweightRestoreReport, LightweightSiteAction,
    LightweightSiteActionReport, LightweightSiteInventory, ProvisionStaticSiteReport,
};
