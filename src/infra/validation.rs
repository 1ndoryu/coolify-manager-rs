/*
 * Validacion de inputs y estados del sistema.
 * Equivale a Validators.psm1 del PowerShell original.
 */

use crate::config::Settings;
use crate::domain::{SiteConfig, StackTemplate};
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

/* [045A-GUARDRAILS] Los stacks Rust guardan entregables e imágenes en /app/uploads.
 * Si sourcePaths se personaliza y omite ese path, el backup pre-deploy queda incompleto
 * y un redeploy puede dejar la app sin archivos recuperables. */
pub fn assert_backup_guardrails(site: &SiteConfig) -> std::result::Result<(), CoolifyError> {
    if !site.backup_policy.enabled || site.template != StackTemplate::Rust {
        return Ok(());
    }

    if site.backup_policy.source_paths.is_empty() {
        return Ok(());
    }

    let has_uploads = site
        .backup_policy
        .source_paths
        .iter()
        .any(|path| path.trim() == "/app/uploads");

    if has_uploads {
        Ok(())
    } else {
        Err(CoolifyError::Validation(format!(
            "ABORT: backupPolicy.sourcePaths para '{}' omite '/app/uploads'. \
             Inclúyelo o deja sourcePaths vacío para usar defaults seguros.",
            site.nombre
        )))
    }
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

    #[test]
    fn rust_backup_guardrails_require_uploads_when_overridden() {
        let mut site = SiteConfig {
            nombre: "studio".to_string(),
            dominio: "https://nakomi.studio".to_string(),
            extra_domains: Vec::new(),
            target: None,
            stack_uuid: Some("uuid-demo".to_string()),
            glory_branch: "main".to_string(),
            library_branch: "main".to_string(),
            theme_name: "glorytheme".to_string(),
            skip_react: false,
            template: StackTemplate::Rust,
            php_config: None,
            smtp_config: None,
            disable_wp_cron: false,
            repo_url: None,
            backup_policy: crate::domain::BackupPolicy {
                enabled: true,
                daily_keep: 2,
                weekly_keep: 3,
                source_paths: vec!["/app/data".to_string()],
            },
            health_check: crate::domain::HealthCheckConfig::default(),
            dns_config: None,
        };

        let err = assert_backup_guardrails(&site).unwrap_err().to_string();
        assert!(err.contains("/app/uploads"));

        site.backup_policy
            .source_paths
            .push("/app/uploads".to_string());
        assert!(assert_backup_guardrails(&site).is_ok());
    }
}

/* [04A-1] M7: Migration linter — verifica que DDL statements usan IF NOT EXISTS.
 * Resuelve E18 (CREATE INDEX sin IF NOT EXISTS → crash loop 42P07).
 * SQLx aborta el startup si una migración falla — no hay skip parcial. */
pub fn lint_migration_sql(sql: &str, filename: &str) -> Vec<String> {
    let mut errors = Vec::new();

    for line in sql.lines() {
        let trimmed = line.trim();
        /* Ignorar comentarios */
        if trimmed.starts_with("--") || trimmed.starts_with("/*") || trimmed.is_empty() {
            continue;
        }

        /* CREATE INDEX sin IF NOT EXISTS */
        let upper = trimmed.to_uppercase();
        if upper.starts_with("CREATE INDEX") && !upper.contains("IF NOT EXISTS") {
            errors.push(format!(
                "{}: CREATE INDEX sin IF NOT EXISTS: '{}'",
                filename, trimmed
            ));
        }
        /* CREATE UNIQUE INDEX sin IF NOT EXISTS */
        if upper.starts_with("CREATE UNIQUE INDEX") && !upper.contains("IF NOT EXISTS") {
            errors.push(format!(
                "{}: CREATE UNIQUE INDEX sin IF NOT EXISTS: '{}'",
                filename, trimmed
            ));
        }
        /* CREATE TABLE sin IF NOT EXISTS */
        if upper.starts_with("CREATE TABLE") && !upper.contains("IF NOT EXISTS") {
            errors.push(format!(
                "{}: CREATE TABLE sin IF NOT EXISTS: '{}'",
                filename, trimmed
            ));
        }
    }

    errors
}

#[cfg(test)]
mod migration_linter_tests {
    use super::*;

    #[test]
    fn test_create_index_requires_if_not_exists() {
        let sql = "CREATE INDEX idx_test ON my_table(col);";
        let errors = lint_migration_sql(sql, "test.sql");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("IF NOT EXISTS"));
    }

    #[test]
    fn test_create_index_with_if_not_exists_is_ok() {
        let sql = "CREATE INDEX IF NOT EXISTS idx_test ON my_table(col);";
        let errors = lint_migration_sql(sql, "test.sql");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_create_table_requires_if_not_exists() {
        let sql = "CREATE TABLE my_table (id SERIAL PRIMARY KEY);";
        let errors = lint_migration_sql(sql, "test.sql");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("CREATE TABLE"));
    }

    #[test]
    fn test_create_table_with_if_not_exists_is_ok() {
        let sql = "CREATE TABLE IF NOT EXISTS my_table (id SERIAL PRIMARY KEY);";
        let errors = lint_migration_sql(sql, "test.sql");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_comments_are_ignored() {
        let sql = "-- CREATE INDEX idx_test ON my_table(col);\nCREATE INDEX IF NOT EXISTS idx_real ON t(c);";
        let errors = lint_migration_sql(sql, "test.sql");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_case_insensitive() {
        let sql = "create index idx_test on my_table(col);";
        let errors = lint_migration_sql(sql, "test.sql");
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn test_real_email_logs_migration() {
        /* E18: La migración que causó el crash loop */
        let sql = r#"
CREATE TABLE IF NOT EXISTS email_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    recipient TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_email_logs_recipient ON email_logs(recipient);
CREATE INDEX IF NOT EXISTS idx_email_logs_status ON email_logs(status);
CREATE INDEX IF NOT EXISTS idx_email_logs_created_at ON email_logs(created_at);
"#;
        let errors = lint_migration_sql(sql, "20260531000000_email_logs.up.sql");
        assert!(
            errors.is_empty(),
            "Migración email_logs debería pasar lint: {:?}",
            errors
        );
    }

    #[test]
    fn test_multiple_errors_in_one_file() {
        let sql = r#"
CREATE TABLE my_table (id INT);
CREATE INDEX idx_a ON my_table(id);
CREATE INDEX IF NOT EXISTS idx_b ON my_table(id);
"#;
        let errors = lint_migration_sql(sql, "bad.sql");
        assert_eq!(errors.len(), 2); /* TABLE + INDEX */
    }
}
