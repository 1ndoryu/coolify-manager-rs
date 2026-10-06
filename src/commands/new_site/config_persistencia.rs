/* SiteConfig persistido en settings.json [06AA-4 split new_site]. */

use super::DatosSitioNuevo;
use crate::domain::{SiteConfig, StackTemplate};

/* Paso 3: construye el SiteConfig con defaults por template. */
pub(super) fn construir_site_config(d: &DatosSitioNuevo<'_>) -> SiteConfig {
    let es_rust = *d.stack_template == StackTemplate::Rust;
    SiteConfig {
        nombre: d.site_name.to_string(),
        dominio: d.domain.to_string(),
        extra_domains: Vec::new(),
        target: if d.target.name == "default" {
            None
        } else {
            Some(d.target.name.clone())
        },
        stack_uuid: Some(d.stack_uuid.to_string()),
        glory_branch: d.glory_branch.to_string(),
        library_branch: d.library_branch.to_string(),
        theme_name: d
            .settings
            .glory
            .default_branch
            .clone()
            .replace("main", "glorytemplate"),
        skip_react: false,
        template: d.stack_template.clone(),
        php_config: None,
        smtp_config: None,
        disable_wp_cron: false,
        repo_url: if es_rust {
            Some(d.valores_rust.repo_url.to_string())
        } else {
            None
        },
        /* [268A-4/5] Para stacks Rust se fijan ya desde `new` (flags --repo-url,
         * --app-bin, --frontend-dir); proyectos no-glory como ong-agape usan
         * ong-agame-backend + frontend-v2 sin tocar settings.json a mano. */
        app_bin: if es_rust {
            d.valores_rust.app_bin.to_string()
        } else {
            crate::domain::default_app_bin()
        },
        frontend_dir: if es_rust {
            d.valores_rust.frontend_dir.to_string()
        } else {
            crate::domain::default_frontend_dir()
        },
        /* [119A-4] Imagen precompilada: deploy-service hará pull en vez de build. */
        image_ref: d.valores_rust.image.map(str::to_string),
        /* [01AA-3] Dónde compila deploy-service para este sitio. */
        build_mode: d.build_mode,
        backup_policy: crate::domain::BackupPolicy::default(),
        /* [B4-1] Los stacks Rust sirven salud en /api/health, no en `/`. */
        health_check: if es_rust {
            crate::domain::HealthCheckConfig::rust_default()
        } else {
            crate::domain::HealthCheckConfig::default()
        },
        dns_config: None,
    }
}
