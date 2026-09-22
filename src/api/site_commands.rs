use crate::api::types::{CreateSiteRequest, OperationResult};
use crate::commands;
use crate::error::CoolifyError;
use std::path::Path;

pub async fn create_site(
    config_path: &Path,
    request: CreateSiteRequest,
) -> Result<OperationResult, CoolifyError> {
    /* [105A-31] La GUI crea sitios reutilizando new_site para conservar validaciones,
     * persistencia en settings.json y flujo Coolify existente. */
    commands::new_site::execute(&commands::new_site::ParamsNewSite {
        config_path,
        site_name: request.name.trim(),
        domain: request.domain.trim(),
        glory_branch: "main",
        library_branch: "main",
        template: request.template.trim(),
        target_name: request
            .target
            .as_deref()
            .filter(|target| !target.trim().is_empty() && target.trim() != "default"),
        repo_url: request.repo_url.as_deref(),
        app_bin: request.app_bin.as_deref(),
        frontend_dir: request.frontend_dir.as_deref(),
        image: request.image.as_deref(),
        skip_theme: request.skip_theme,
        skip_cache: request.skip_cache,
    })
    .await?;

    Ok(OperationResult {
        success: true,
        message: format!("Sitio '{}' creado", request.name.trim()),
        details: Some(format!(
            "Dominio: {}\nTemplate: {}",
            request.domain.trim(),
            request.template.trim()
        )),
    })
}
