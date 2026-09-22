/* Split 119A-5 de target_bootstrap_manager.rs — reportes y estado interno.
 * Re-exportados sin cambios desde target_bootstrap_manager/mod.rs. */

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct InstallCoolifyReport {
    pub target: String,
    pub access_url: String,
    pub os_name: String,
    pub already_installed: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BootstrapLightTargetReport {
    pub target: String,
    pub dry_run: bool,
    pub services_ready: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UninstallCoolifyReport {
    pub target: String,
    pub dry_run: bool,
    pub purge_data: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct CoolifyResourceState {
    pub(super) containers: Vec<String>,
    pub(super) volumes: Vec<String>,
    pub(super) networks: Vec<String>,
    pub(super) data_path_exists: bool,
}

#[derive(Debug, Clone)]
pub(super) struct LightRuntimeState {
    pub(super) docker_installed: bool,
    pub(super) docker_active: bool,
    pub(super) caddy_installed: bool,
    pub(super) caddy_active: bool,
    pub(super) mariadb_installed: bool,
    pub(super) mariadb_active: bool,
    pub(super) redis_installed: bool,
    pub(super) redis_active: bool,
    pub(super) hosting_root_exists: bool,
    pub(super) backups_root_exists: bool,
    pub(super) caddy_sites_dir_exists: bool,
}
