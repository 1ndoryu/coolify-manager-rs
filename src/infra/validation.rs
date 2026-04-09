/*
 * Validacion de inputs y estados del sistema.
 * Equivale a Validators.psm1 del PowerShell original.
 */

use crate::config::Settings;
use crate::domain::SiteConfig;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;

/// Valida formato de dominio (requiere protocolo http/https).
pub fn validate_domain(domain: &str) -> std::result::Result<(), CoolifyError> {
    if domain.is_empty() {
        return Err(CoolifyError::Validation(
            "Dominio no puede estar vacio".into(),
        ));
    }
    if !domain.starts_with("http://") && !domain.starts_with("https://") {
        return Err(CoolifyError::Validation(format!(
            "Dominio '{domain}' debe incluir protocolo (https://...)"
        )));
    }
    if domain.contains(' ') {
        return Err(CoolifyError::Validation(format!(
            "Dominio '{domain}' no puede contener espacios"
        )));
    }
    Ok(())
}

/// Valida que el nombre del sitio sea un slug valido.
pub fn validate_site_name(name: &str) -> std::result::Result<(), CoolifyError> {
    if name.is_empty() {
        return Err(CoolifyError::Validation(
            "Nombre de sitio no puede estar vacio".into(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(CoolifyError::Validation(format!(
            "Nombre de sitio '{name}' solo puede contener letras, numeros, guiones y guiones bajos"
        )));
    }
    Ok(())
}

/// Verifica que un sitio tenga stackUuid asignado.
pub fn assert_site_ready(site: &SiteConfig) -> std::result::Result<(), CoolifyError> {
    if site.stack_uuid.is_none() {
        return Err(CoolifyError::Validation(format!(
            "Sitio '{}' no tiene stackUuid asignado. Ejecuta 'new' primero.",
            site.nombre
        )));
    }
    Ok(())
}

/// Valida que un archivo exista en disco.
pub fn validate_file_exists(path: &std::path::Path) -> std::result::Result<(), CoolifyError> {
    if !path.exists() {
        return Err(CoolifyError::Validation(format!(
            "Archivo no encontrado: {}",
            path.display()
        )));
    }
    if !path.is_file() {
        return Err(CoolifyError::Validation(format!(
            "La ruta no es un archivo: {}",
            path.display()
        )));
    }
    Ok(())
}

/* [F2] Pre-deploy safety check: verifica que todos los sitios configurados siguen existiendo
 * en Coolify. Previene el escenario donde un deploy destruye servicios de otros sitios
 * sin que nadie se entere hasta que es demasiado tarde. */
pub async fn pre_deploy_safety_check(
    settings: &Settings,
    target_site: &str,
) -> std::result::Result<(), CoolifyError> {
    let site = settings.get_site(target_site)?;
    let target = settings.resolve_site_target(site)?;
    let api = CoolifyApiClient::new(&target.coolify)?;
    let mut missing: Vec<String> = Vec::new();

    for s in &settings.sitios {
        let uuid = match &s.stack_uuid {
            Some(u) if !u.is_empty() => u,
            _ => continue,
        };
        /* Solo verificar sitios del mismo servidor */
        let s_target = match settings.resolve_site_target(s) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if s_target.vps.ip != target.vps.ip {
            continue;
        }
        match api.get_service(uuid).await {
            Ok(_) => {}
            Err(_) => {
                missing.push(format!("{} (uuid={})", s.nombre, uuid));
            }
        }
    }

    if !missing.is_empty() {
        return Err(CoolifyError::Validation(format!(
            "ABORT: {} sitio(s) no encontrado(s) en Coolify ANTES del deploy: {}. \
             Investiga antes de continuar.",
            missing.len(),
            missing.join(", ")
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_domain_valid() {
        assert!(validate_domain("https://blog.com").is_ok());
        assert!(validate_domain("http://localhost:8080").is_ok());
        assert!(validate_domain("https://sub.domain.co.uk").is_ok());
    }

    #[test]
    fn test_validate_domain_missing_protocol() {
        let result = validate_domain("blog.com");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("protocolo"));
    }

    #[test]
    fn test_validate_domain_empty() {
        assert!(validate_domain("").is_err());
    }

    #[test]
    fn test_validate_domain_spaces() {
        assert!(validate_domain("https://my site.com").is_err());
    }

    #[test]
    fn test_validate_site_name_valid() {
        assert!(validate_site_name("blog").is_ok());
        assert!(validate_site_name("mi-sitio").is_ok());
        assert!(validate_site_name("site_01").is_ok());
    }

    #[test]
    fn test_validate_site_name_invalid() {
        assert!(validate_site_name("").is_err());
        assert!(validate_site_name("site with spaces").is_err());
        assert!(validate_site_name("site@special").is_err());
    }

    #[test]
    fn test_validate_file_exists_nonexistent() {
        let result = validate_file_exists(std::path::Path::new("/nonexistent/file.sql"));
        assert!(result.is_err());
    }
}
