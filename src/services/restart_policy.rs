/*
 * Politica de reinicio por la API de Coolify.
 *
 * [310A-1] Decide, en un solo sitio, si un sitio sobrevive a un restart por la
 * API cruda de Coolify. Antes esta regla vivia duplicada dentro de
 * `restart_site`, y `env_toggle --restart` la esquivaba llamando a la API
 * directamente: el 2026-10-18 a las 23:33 CEST eso dejo `inmobiliaria` sin
 * contenedores (Coolify para y borra el servicio, y no puede recrear una
 * imagen que solo existe en el host: `CoolifyTask` FAIL y compose pinchando
 * `cm-local/inmobiliaria:254bfe52e429`, tag ya ausente).
 * Regla: si el sitio no es reiniciable por API, se para; no se intenta.
 */

use crate::domain::{SiteConfig, StackTemplate};

/* [01AA-1] Las imagenes `cm-local/` se construyen en el portatil y viajan al
 * VPS por SSH: no estan en ningun registry, asi que Coolify no puede
 * recuperarlas despues de un `down`. */
pub fn usa_imagen_local_cm(image_ref: Option<&str>) -> bool {
    image_ref.is_some_and(|r| r.starts_with("cm-local/"))
}

/// Motivo por el que `site` NO se puede reiniciar con la API cruda de Coolify,
/// o `None` si el reinicio es seguro.
pub fn motivo_no_reiniciable_api(site: &SiteConfig) -> Option<String> {
    /* [115A-STUDIO-AUTOHEAL] Rust usa imagen local: el restart por API lo deja
     * en exited sin poder levantarlo. */
    if matches!(site.template, StackTemplate::Rust) {
        return Some(format!(
            "'{}' es un sitio Rust: la API de Coolify lo para y no puede volver a levantarlo sin imagen. Usa 'deploy-service --name {}'.",
            site.nombre, site.nombre
        ));
    }
    if usa_imagen_local_cm(site.image_ref.as_deref()) {
        return Some(format!(
            "'{}' usa la imagen local {}: al recrear el servicio, Coolify la perderia (no esta en ningun registry). Reconstruye con build-laptop + new/deploy-service --image.",
            site.nombre,
            site.image_ref.as_deref().unwrap_or("?")
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sitio(nombre: &str, template: StackTemplate, image_ref: Option<&str>) -> SiteConfig {
        SiteConfig {
            nombre: nombre.to_string(),
            dominio: "https://ejemplo.test".to_string(),
            extra_domains: Vec::new(),
            target: None,
            stack_uuid: Some("stack".to_string()),
            glory_branch: "main".to_string(),
            library_branch: "main".to_string(),
            theme_name: "glory".to_string(),
            skip_react: false,
            template,
            php_config: None,
            smtp_config: None,
            disable_wp_cron: false,
            backup_policy: crate::domain::BackupPolicy::default(),
            health_check: crate::domain::HealthCheckConfig::default(),
            dns_config: None,
            repo_url: None,
            repo_local: None,
            app_bin: crate::domain::default_app_bin(),
            frontend_dir: crate::domain::default_frontend_dir(),
            image_ref: image_ref.map(str::to_string),
            build_mode: Default::default(),
        }
    }

    #[test]
    fn detecta_imagen_local_cm() {
        assert!(usa_imagen_local_cm(Some("cm-local/kamples:abc123def456")));
        assert!(!usa_imagen_local_cm(Some("ghcr.io/org/app:1.2.3")));
        assert!(!usa_imagen_local_cm(None));
        assert!(!usa_imagen_local_cm(Some("cm-local")));
    }

    /* El caso del incidente 2026-10-18: Rust + cm-local a la vez. */
    #[test]
    fn rust_con_imagen_local_no_es_reiniciable() {
        let s = sitio(
            "inmobiliaria",
            StackTemplate::Rust,
            Some("cm-local/inmobiliaria:254bfe52e429"),
        );
        assert!(motivo_no_reiniciable_api(&s).is_some());
    }

    #[test]
    fn rust_sin_imagen_local_tampoco_es_reiniciable() {
        let s = sitio("rust-solo", StackTemplate::Rust, None);
        assert!(motivo_no_reiniciable_api(&s).is_some());
    }

    #[test]
    fn imagen_local_en_template_no_rust_no_es_reiniciable() {
        let s = sitio(
            "kamples",
            StackTemplate::Kamples,
            Some("cm-local/kamples:d2993e7caf42"),
        );
        assert!(motivo_no_reiniciable_api(&s).is_some());
    }

    #[test]
    fn imagen_de_registry_si_es_reiniciable() {
        let s = sitio(
            "wordpress",
            StackTemplate::Wordpress,
            Some("ghcr.io/org/app:1.2.3"),
        );
        assert!(motivo_no_reiniciable_api(&s).is_none());
    }
}
