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

/// Valida que un segmento de ruta aportado por input externo no permita traversal.
/// Solo letras, numeros, guion, guion bajo y punto simple; rechaza `/`, `\`,
/// `..` y segmentos vacios. Es la contraparte de `validate_site_name` para
/// nombres de tarea, timer, fichero y credencial que tambien acaban en `join`.
pub fn validar_segmento_ruta(segmento: &str, campo: &str) -> std::result::Result<(), CoolifyError> {
    if segmento.is_empty() {
        return Err(CoolifyError::Validation(format!(
            "{campo} no puede estar vacio"
        )));
    }
    if segmento.contains('/')
        || segmento.contains('\\')
        || segmento.contains("..")
        || segmento.contains('\0')
    {
        return Err(CoolifyError::Validation(format!(
            "{campo} '{segmento}' no puede contener separadores ni '..'"
        )));
    }
    if !segmento
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(CoolifyError::Validation(format!(
            "{campo} '{segmento}' solo puede contener letras, numeros, '-', '_' y '.'"
        )));
    }
    Ok(())
}

/* [119A-5 Lote A] join seguro contra traversal.
 * Valida el segmento y, cuando el resultado existe en disco, lo canonicalize()
 * y verifica que siga bajo `base` con starts_with(). Si aun no existe
 * (pendiente de create_dir_all/write), la validacion sintactica previa ya
 * impide `..` y separadores, que es el vector real de escape. */
pub fn join_segmento_seguro(
    base: &std::path::Path,
    segmento: &str,
    campo: &str,
) -> std::result::Result<std::path::PathBuf, CoolifyError> {
    validar_segmento_ruta(segmento, campo)?;
    let ruta = base.join(segmento);
    if let Ok(canon_base) = base.canonicalize() {
        if let Ok(canon_ruta) = ruta.canonicalize() {
            if !canon_ruta.starts_with(&canon_base) {
                return Err(CoolifyError::Validation(format!(
                    "{campo} '{segmento}' escapa del directorio base"
                )));
            }
        }
    }
    Ok(ruta)
}

/* [119A-5 Lote A2] relativo seguro para manifests (puede contener subdirs).
 * A diferencia del segmento simple, permite `/` pero rechaza `..`, `\0`,
 * paths absolutos y `~`. Tras el join, si el resultado existe lo
 * canonicalize() y exige starts_with() bajo `base`. */
pub fn validar_ruta_relativa(relativo: &str, campo: &str) -> std::result::Result<(), CoolifyError> {
    if relativo.is_empty() {
        return Err(CoolifyError::Validation(format!(
            "{campo} no puede estar vacio"
        )));
    }
    if relativo.contains('\0') || relativo.contains("..") || relativo.starts_with('~') {
        return Err(CoolifyError::Validation(format!(
            "{campo} '{relativo}' no puede contener '..' ni caracteres nulos"
        )));
    }
    let ruta = std::path::Path::new(relativo);
    if ruta.is_absolute() {
        return Err(CoolifyError::Validation(format!(
            "{campo} '{relativo}' debe ser relativo, no absoluto"
        )));
    }
    Ok(())
}

pub fn unir_relativo_seguro(
    base: &std::path::Path,
    relativo: &str,
    campo: &str,
) -> std::result::Result<std::path::PathBuf, CoolifyError> {
    validar_ruta_relativa(relativo, campo)?;
    let ruta = base.join(relativo);
    if let Ok(canon_base) = base.canonicalize() {
        if let Ok(canon_ruta) = ruta.canonicalize() {
            if !canon_ruta.starts_with(&canon_base) {
                return Err(CoolifyError::Validation(format!(
                    "{campo} '{relativo}' escapa del directorio base"
                )));
            }
        }
    }
    Ok(ruta)
}

/// Verifica que un sitio tenga stackUuid asignado.
pub fn assert_site_ready(site: &SiteConfig) -> std::result::Result<(), CoolifyError> {
    if site.stack_uuid.is_none() {
        let nombre = &site.nombre;
        return Err(CoolifyError::Validation(format!(
            "Sitio '{nombre}' no tiene stackUuid asignado. Ejecuta 'new' primero."
        )));
    }
    Ok(())
}

/// [119A-4] Valida una referencia de imagen de registry (`registry/owner/app:tag`).
/// Política de tag fijo: exige tag explícito y rechaza `:latest` (mutable, sin
/// rollback fiable). También exige ASCII puro (Coolify beta.460, mismo motivo
/// que el compose en [268A-5]).
pub fn validate_image_ref(image_ref: &str) -> std::result::Result<(), CoolifyError> {
    if image_ref.is_empty() {
        return Err(CoolifyError::Validation(
            "Referencia de imagen no puede estar vacia (formato registry/owner/app:tag)".into(),
        ));
    }
    if !image_ref.is_ascii() {
        return Err(CoolifyError::Validation(format!(
            "Referencia de imagen '{image_ref}' debe ser ASCII puro"
        )));
    }
    if image_ref.contains(' ') {
        return Err(CoolifyError::Validation(format!(
            "Referencia de imagen '{image_ref}' no puede contener espacios"
        )));
    }
    let (repo, tag) = image_ref.rsplit_once(':').ok_or_else(|| {
        CoolifyError::Validation(format!(
            "Referencia de imagen '{image_ref}' debe incluir tag fijo (p. ej. ghcr.io/1ndoryu/app:abc1234)"
        ))
    })?;
    if !repo.contains('/') {
        return Err(CoolifyError::Validation(format!(
            "Referencia de imagen '{image_ref}' debe incluir registry y owner (registry/owner/app:tag)"
        )));
    }
    if tag.is_empty() {
        return Err(CoolifyError::Validation(format!(
            "Referencia de imagen '{image_ref}' tiene tag vacio"
        )));
    }
    if tag.eq_ignore_ascii_case("latest") {
        return Err(CoolifyError::Validation(format!(
            "Referencia de imagen '{image_ref}' usa tag 'latest' (mutable): fija un tag de version o sha para rollback fiable"
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

    /* [119A-4] Referencia de imagen con tag fijo. */
    #[test]
    fn test_validate_image_ref_ok() {
        assert!(validate_image_ref("ghcr.io/1ndoryu/task:abc1234").is_ok());
        assert!(validate_image_ref("ghcr.io/1ndoryu/task:v1.2.3").is_ok());
    }

    #[test]
    fn test_validate_image_ref_rechaza_latest() {
        let result = validate_image_ref("ghcr.io/1ndoryu/task:latest");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("latest"));
    }

    #[test]
    fn test_validate_image_ref_rechaza_sin_tag() {
        assert!(validate_image_ref("ghcr.io/1ndoryu/task").is_err());
        assert!(validate_image_ref("ghcr.io/1ndoryu/task:").is_err());
        assert!(validate_image_ref("task:abc1234").is_err());
    }

    #[test]
    fn test_validate_image_ref_rechaza_espacios_y_no_ascii() {
        assert!(validate_image_ref("ghcr.io/1ndoryu/mi app:abc").is_err());
        assert!(validate_image_ref("ghcr.io/1ndoryu/taréa:abc").is_err());
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
            app_bin: crate::domain::default_app_bin(),
            frontend_dir: crate::domain::default_frontend_dir(),
            image_ref: None,
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
