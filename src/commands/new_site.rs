/*
 * Comando: new-site
 * Crea un nuevo sitio WordPress con tema Glory en Coolify.
 * Flujo: validar → crear stack → esperar ready → instalar tema → activar tema → cache.
 */

use crate::config::Settings;
use crate::domain::{SiteConfig, StackTemplate};
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::ssh_client::SshClient;
use crate::infra::template_engine;
use crate::infra::validation;
use crate::services::{cache_manager, site_manager, theme_manager};

use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub async fn execute(
    config_path: &Path,
    site_name: &str,
    domain: &str,
    glory_branch: &str,
    library_branch: &str,
    template: &str,
    target_name: Option<&str>,
    repo_url: Option<&str>,
    app_bin: Option<&str>,
    frontend_dir: Option<&str>,
    /* [119A-4] Imagen precompilada (registry/owner/app:tag). Si se pasa,
     * el stack usa el template rust-image (pull) en vez de compilar. */
    image: Option<&str>,
    skip_theme: bool,
    skip_cache: bool,
) -> std::result::Result<(), CoolifyError> {
    /* Validaciones + carga de settings/target + deteccion de placeholder. */
    let (mut settings, target, es_placeholder) =
        cargar_target_y_placeholder(config_path, site_name, domain, image, target_name)?;

    let stack_template: StackTemplate = match template {
        "kamples" => StackTemplate::Kamples,
        "minecraft" => StackTemplate::Minecraft,
        "rust" => StackTemplate::Rust,
        _ => StackTemplate::Wordpress,
    };

    /* [268A-5] Los stacks que referencian un Dockerfile EXTERNO en disco
     * (Rust: `dockerfile: Dockerfile.rust`; Kamples usa dockerfile_inline pero
     * se trata igual por seguridad) se crean SIN instant_deploy: el Dockerfile
     * aún no está en /data/coolify/services/{uuid} y `docker compose up --build`
     * fallaría al instante. Tras `new` hay que ejecutar `deploy-service`, que
     * sube el Dockerfile, sincroniza el compose y construye con health check. */
    let necesita_deploy_service =
        matches!(stack_template, StackTemplate::Rust | StackTemplate::Kamples);

    tracing::info!("Creando sitio '{site_name}' con dominio {domain} (template: {template})");

    /* [268A-5] Valores efectivos del stack Rust: flags CLI > defaults.
     * Se guardan en settings.json para que el sitio quede correcto desde el
     * primer deploy (antes había que editar settings.json a mano). */
    let resolved_repo_url = repo_url
        .unwrap_or("https://github.com/1ndoryu/glory-rs.git")
        .to_string();
    let resolved_app_bin = app_bin
        .map(str::to_string)
        .unwrap_or_else(crate::domain::default_app_bin);
    let resolved_frontend_dir = frontend_dir
        .map(str::to_string)
        .unwrap_or_else(crate::domain::default_frontend_dir);

    /* Paso 1: Generar Docker Compose desde template */
    let valores_rust = ValoresRust {
        repo_url: &resolved_repo_url,
        app_bin: &resolved_app_bin,
        frontend_dir: &resolved_frontend_dir,
        image,
    };
    let compose_yaml = generar_compose(
        config_path,
        &settings,
        site_name,
        domain,
        &stack_template,
        glory_branch,
        library_branch,
        &valores_rust,
    )?;


    /* [119A-5] canonicalize: generar_compose resuelve el template desde
     * templates/ con segmento validado (enum StackTemplate o literal fijo). */

    /* Paso 2: Crear stack en Coolify */
    let api = CoolifyApiClient::new(&target.coolify)?;
    let stack_result = api
        .create_stack(
            site_name,
            &target.coolify.server_uuid,
            &target.coolify.project_uuid,
            &target.coolify.environment_name,
            &compose_yaml,
            !necesita_deploy_service,
        )
        .await?;

    tracing::info!(
        "Stack creado: uuid={}, name={}",
        stack_result.uuid,
        stack_result.name
    );

    /* [25A-DB-AUTH] Reemplazar STACK_UUID_PLACEHOLDER con el UUID real para evitar colisión DNS
     * en la red compartida coolify. El template usa postgres-{{STACK_UUID}} en DATABASE_URL
     * y container_name, pero el UUID solo está disponible después de create_stack(). */
    fijar_uuid_en_compose(&api, &compose_yaml, &stack_result.uuid).await;

    /* Paso 3: Guardar sitio en configuracion */
    let datos = DatosSitioNuevo {
        site_name,
        domain,
        target: &target,
        stack_uuid: &stack_result.uuid,
        glory_branch,
        library_branch,
        stack_template: &stack_template,
        settings: &settings,
        valores_rust: &valores_rust,
    };
    let site_config = construir_site_config(&datos);
    if es_placeholder {
        settings.update_site(site_config, config_path)?;
    } else {
        settings.add_site(site_config, config_path)?;
    }

    /* Paso 4: Esperar a que los contenedores esten listos */
    tracing::info!("Esperando a que el stack este listo...");
    tokio::time::sleep(std::time::Duration::from_secs(30)).await;

    /* Paso 5: Conectar SSH e instalar tema */
    let es_wordpress = matches!(
        stack_template,
        StackTemplate::Wordpress | StackTemplate::Kamples
    );
    if !skip_theme && es_wordpress {
        instalar_tema_wordpress(
            &settings,
            &target,
            &stack_result.uuid,
            glory_branch,
            library_branch,
            domain,
            skip_cache,
        )
        .await?;
    }

    imprimir_resumen(
        site_name,
        domain,
        &target.name,
        &stack_result.uuid,
        necesita_deploy_service,
        image,
    );
    Ok(())
}

/* Validaciones + carga de settings/target + deteccion de placeholder. */
fn cargar_target_y_placeholder(
    config_path: &Path,
    site_name: &str,
    domain: &str,
    image: Option<&str>,
    target_name: Option<&str>,
) -> std::result::Result<
    (
        Settings,
        crate::config::DeploymentTargetConfig,
        bool,
    ),
    CoolifyError,
> {
    validation::validate_site_name(site_name)?;
    validation::validate_domain(domain)?;
    if let Some(image_ref) = image {
        validation::validate_image_ref(image_ref)?;
    }

    let settings = Settings::load(config_path)?;
    let target = match target_name {
        Some(name) => settings.get_target(name)?.clone(),
        None => settings.default_target(),
    };

    /* Verificar que el sitio no existe o es un placeholder (stackUuid vacio) */
    let es_placeholder = if let Some(existing) =
        settings.sitios.iter().find(|s| s.nombre == site_name)
    {
        if existing.stack_uuid.as_ref().is_some_and(|u| !u.is_empty()) {
            return Err(CoolifyError::Validation(format!(
                "El sitio '{site_name}' ya existe con stack activo (uuid: {})",
                existing.stack_uuid.as_deref().unwrap_or("")
            )));
        }
        tracing::info!(
            "Sitio '{site_name}' existe como placeholder, se actualizara con el nuevo stack"
        );
        true
    } else {
        false
    };
    Ok((settings, target, es_placeholder))
}

/* [25A-DB-AUTH] Sustituye STACK_UUID_PLACEHOLDER por el UUID real post-create. */
async fn fijar_uuid_en_compose(
    api: &CoolifyApiClient,
    compose_yaml: &str,
    stack_uuid: &str,
) {
    if !compose_yaml.contains("STACK_UUID_PLACEHOLDER") {
        return;
    }
    let fixed_compose = compose_yaml.replace("STACK_UUID_PLACEHOLDER", stack_uuid);
    tracing::info!(
        "Actualizando compose con UUID real ({}) para evitar colision DNS...",
        stack_uuid
    );
    if let Err(e) = api.update_stack_compose(stack_uuid, &fixed_compose).await {
        tracing::warn!("No se pudo actualizar compose con UUID real: {e}. Ejecuta fix-db-auth tras el primer deploy.");
    } else {
        tracing::info!(
            "Compose actualizado: DATABASE_URL ahora usa postgres-{}",
            stack_uuid
        );
    }
}

/* Resumen final + siguiente paso obligatorio para templates con build. */
fn imprimir_resumen(
    site_name: &str,
    domain: &str,
    target_name: &str,
    stack_uuid: &str,
    necesita_deploy_service: bool,
    image: Option<&str>,
) {
    println!("Sitio '{site_name}' creado exitosamente.");
    println!("  Dominio: {domain}");
    println!("  Target: {target_name}");
    println!("  Stack UUID: {stack_uuid}");
    if necesita_deploy_service {
        println!();
        println!("  SIGUIENTE PASO (obligatorio para templates con build):");
        println!("    deploy-service --name {site_name} --skip-backup");
        if image.is_some() {
            println!("  (sincroniza compose y descarga la imagen precompilada: sin build en VPS)");
        } else {
            println!(
                "  (sube el Dockerfile al directorio del servicio, sincroniza compose y construye)"
            );
        }
    }
}

/* Valores efectivos del stack Rust: flags CLI > defaults. */
struct ValoresRust<'a> {
    repo_url: &'a str,
    app_bin: &'a str,
    frontend_dir: &'a str,
    image: Option<&'a str>,
}

/* Paso 1: genera vars segun template y renderiza el compose desde templates/. */
fn generar_compose(
    config_path: &Path,
    settings: &Settings,
    site_name: &str,
    domain: &str,
    stack_template: &StackTemplate,
    glory_branch: &str,
    library_branch: &str,
    rust: &ValoresRust<'_>,
) -> std::result::Result<String, CoolifyError> {
    let db_password = template_engine::generate_password(24);
    let root_password = template_engine::generate_password(24);
    let compose_vars = match stack_template {
        StackTemplate::Wordpress => template_engine::wordpress_vars(
            domain,
            &db_password,
            &root_password,
            &settings.glory.template_repo,
            &settings.glory.library_repo,
            glory_branch,
            library_branch,
            "glorytemplate",
        ),
        StackTemplate::Kamples => {
            let pg_password = template_engine::generate_password(24);
            template_engine::kamples_vars(
                domain,
                &db_password,
                &root_password,
                &pg_password,
                glory_branch,
                &settings.glory.template_repo,
                &settings.glory.library_repo,
                library_branch,
                "glorytemplate",
            )
        }
        StackTemplate::Minecraft => template_engine::minecraft_vars(site_name),
        /* [119A-4] Con --image el stack Rust usa el template por imagen
         * (pull desde registry, sin build en la VPS). */
        StackTemplate::Rust if rust.image.is_some() => template_engine::with_image_ref(
            template_engine::rust_vars_full(
                domain,
                glory_branch,
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
            glory_branch,
            rust.repo_url,
            site_name,
            &[],
            rust.app_bin,
            rust.frontend_dir,
        ),
    };

    /* [119A-5] canonicalize: nombre de template del enum StackTemplate o literal fijo. */
    let template_name = if *stack_template == StackTemplate::Rust && rust.image.is_some() {
        "rust-image-stack.yaml".to_string()
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

/* Datos para construir el SiteConfig persistido en settings.json. */
struct DatosSitioNuevo<'a> {
    site_name: &'a str,
    domain: &'a str,
    target: &'a crate::config::DeploymentTargetConfig,
    stack_uuid: &'a str,
    glory_branch: &'a str,
    library_branch: &'a str,
    stack_template: &'a StackTemplate,
    settings: &'a Settings,
    valores_rust: &'a ValoresRust<'a>,
}

/* Paso 3: construye el SiteConfig con defaults por template. */
fn construir_site_config(d: &DatosSitioNuevo<'_>) -> SiteConfig {
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

    let wp_container =
        crate::infra::docker::find_wordpress_container(&ssh, stack_uuid).await?;

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
