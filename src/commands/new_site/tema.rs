/* Paso 5: instalación del tema Glory via SSH [06AA-4 split new_site]. */

use super::Ramas;
use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::services::{cache_manager, site_manager, theme_manager};

/* Paso 5: instala el tema WP via SSH (solo se llama si skip_theme=false y es WP/Kamples). */
pub(super) async fn instalar_tema_si_wordpress(
    settings: &Settings,
    target: &crate::config::DeploymentTargetConfig,
    stack_uuid: &str,
    ramas: &Ramas<'_>,
    domain: &str,
    skip_cache: bool,
) -> std::result::Result<(), CoolifyError> {
    instalar_tema_wordpress(
        settings,
        target,
        stack_uuid,
        ramas.glory_branch,
        ramas.library_branch,
        domain,
        skip_cache,
    )
    .await
}

/* Paso 5: SSH + instalar/activar tema + URLs + cache headers. */
async fn instalar_tema_wordpress(
    settings: &Settings,
    target: &crate::config::DeploymentTargetConfig,
    stack_uuid: &str,
    glory_branch: &str,
    library_branch: &str,
    domain: &str,
    skip_cache: bool,
) -> std::result::Result<(), CoolifyError> {
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let wp_container = crate::infra::docker::find_wordpress_container(&ssh, stack_uuid).await?;

    /* Instalar tema Glory */
    theme_manager::install_glory_theme(
        &ssh,
        &wp_container,
        &settings.glory,
        glory_branch,
        library_branch,
        "glorytemplate",
        false,
    )
    .await?;

    /* Activar tema */
    site_manager::enable_glory_theme(&ssh, &wp_container, "glorytemplate").await?;

    /* Configurar URLs */
    site_manager::set_wordpress_urls(&ssh, &wp_container, domain).await?;

    /* Cache headers */
    if !skip_cache {
        cache_manager::enable_cache_headers(&ssh, &wp_container).await?;
    }
    Ok(())
}
