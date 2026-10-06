/* Render del compose desde templates/ según template [06AA-4 split new_site]. */

use super::{Ramas, ValoresRust};
use crate::config::Settings;
use crate::domain::StackTemplate;
use crate::error::CoolifyError;
use crate::infra::template_engine;
use crate::infra::validation;

use std::path::Path;

/* Paso 1: genera vars segun template y renderiza el compose desde templates/. */
pub(super) fn generar_compose(
    config_path: &Path,
    settings: &Settings,
    site_name: &str,
    domain: &str,
    stack_template: &StackTemplate,
    ramas: &Ramas<'_>,
    rust: &ValoresRust<'_>,
) -> std::result::Result<String, CoolifyError> {
    let db_password = template_engine::generate_password(24);
    let root_password = template_engine::generate_password(24);
    /* VarsTema agrupa los 8 parámetros del tema (119A-6). */
    let tema = template_engine::VarsTema {
        domain,
        db_password: &db_password,
        root_password: &root_password,
        theme_repo: &settings.glory.template_repo,
        library_repo: &settings.glory.library_repo,
        glory_branch: ramas.glory_branch,
        library_branch: ramas.library_branch,
        theme_name: "glorytemplate",
    };
    let compose_vars = match stack_template {
        StackTemplate::Wordpress => template_engine::wordpress_vars(&tema),
        /* [309A-1/F3] Kamples con --image: compose por imagen + tema
         * post-arranque (Paso 5). Sin esta rama caería al template con
         * build embebido y la VPS compilaría en el deploy inicial. */
        StackTemplate::Kamples if rust.image.is_some() => {
            let pg_password = template_engine::generate_password(24);
            template_engine::with_image_ref(
                template_engine::kamples_vars(&template_engine::VarsKamples {
                    base: tema,
                    pg_password: &pg_password,
                }),
                rust.image.unwrap_or_default(),
            )
        }
        StackTemplate::Kamples => {
            let pg_password = template_engine::generate_password(24);
            template_engine::kamples_vars(&template_engine::VarsKamples {
                base: tema,
                pg_password: &pg_password,
            })
        }
        StackTemplate::Minecraft => template_engine::minecraft_vars(site_name),
        /* [119A-4] Con --image el stack Rust usa el template por imagen
         * (pull desde registry, sin build en la VPS). */
        StackTemplate::Rust if rust.image.is_some() => template_engine::with_image_ref(
            template_engine::rust_vars_full(
                domain,
                ramas.glory_branch,
                rust.repo_url,
                site_name,
                &[],
                rust.app_bin,
                rust.frontend_dir,
            ),
            rust.image.unwrap_or_default(),
        ),
        StackTemplate::Rust => template_engine::rust_vars_full(
            domain,
            ramas.glory_branch,
            rust.repo_url,
            site_name,
            &[],
            rust.app_bin,
            rust.frontend_dir,
        ),
    };

    /* [119A-5] canonicalize: nombre de template del enum StackTemplate o literal fijo.
     * [309A-1/F3] Kamples con --image usa su propio template por imagen. */
    let template_name = if *stack_template == StackTemplate::Rust && rust.image.is_some() {
        "rust-image-stack.yaml".to_string()
    } else if *stack_template == StackTemplate::Kamples && rust.image.is_some() {
        "kamples-image-stack.yaml".to_string()
    } else {
        format!("{}-stack.yaml", stack_template)
    };
    validation::validar_segmento_ruta(&template_name, "template")?;
    let templates_base = config_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("templates");
    let template_file =
        validation::join_segmento_seguro(&templates_base, &template_name, "template")?;

    if template_file.exists() {
        template_engine::render_file(&template_file, &compose_vars)
    } else {
        tracing::warn!("Template {template_file:?} no encontrado, usando compose basico");
        Ok(format!(
            "# Stack generado para {site_name}\n# Template no disponible, crear manualmente"
        ))
    }
}
