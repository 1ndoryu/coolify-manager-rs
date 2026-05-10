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
    commands::new_site::execute(
        config_path,
        request.name.trim(),
        request.domain.trim(),
        "main",
        "main",
        request.template.trim(),
        request
            .target
            .as_deref()
            .filter(|target| !target.trim().is_empty() && target.trim() != "default"),
        request.skip_theme,
        request.skip_cache,
    )
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
