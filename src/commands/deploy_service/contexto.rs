/* Contexto compartido de las fases del deploy (119A-5 split deploy_service). */

/* [F1] Contexto del deploy: agrupa los parametros que las fases comparten para
 * evitar clippy::too-many-arguments en cada fase. */
pub(super) struct CtxDeploy<'a> {
    pub(super) settings: &'a crate::config::Settings,
    pub(super) site: &'a crate::domain::SiteConfig,
    pub(super) site_name: &'a str,
    pub(super) config_path: &'a std::path::Path,
    pub(super) target: crate::config::DeploymentTargetConfig,
    pub(super) service_dir: String,
    pub(super) compose_service: String,
    pub(super) stack_uuid: &'a str,
    pub(super) skip_build: bool,
    pub(super) skip_compose_sync: bool,
    pub(super) seed: bool,
}
