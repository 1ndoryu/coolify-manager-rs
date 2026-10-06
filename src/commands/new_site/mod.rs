/*
 * Comando: new-site
 * Crea un nuevo sitio WordPress con tema Glory en Coolify.
 * Flujo: validar → crear stack → esperar ready → reconciliar DB-auth
 * → instalar tema → activar tema → cache.
 *
 * [06AA-4] Split por dominio: `compose_render` (Paso 1),
 * `config_persistencia` (Paso 3), `espera_db` (Pasos 4/4.5) y `tema`
 * (Paso 5). Superficie pública intacta (execute + ParamsNewSite).
 */

mod compose_render;
mod config_persistencia;
mod espera_db;
mod tema;

use compose_render::generar_compose;
use config_persistencia::construir_site_config;
use espera_db::esperar_y_reconciliar_db;
use tema::instalar_tema_si_wordpress;

use crate::config::Settings;
use crate::domain::{BuildMode, SiteConfig, StackTemplate};
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::validation;

use std::path::Path;

/* Params del comando new-site (119A-6: agrupa los 13 flags de execute).
 * Estructura Copy: el body los copia con `= *p` sin mover. */
#[derive(Clone, Copy)]
pub struct ParamsNewSite<'a> {
    pub config_path: &'a Path,
    pub site_name: &'a str,
    pub domain: &'a str,
    pub glory_branch: &'a str,
    pub library_branch: &'a str,
    pub template: &'a str,
    pub target_name: Option<&'a str>,
    pub repo_url: Option<&'a str>,
    pub app_bin: Option<&'a str>,
    pub frontend_dir: Option<&'a str>,
    /* [119A-4] Imagen precompilada (registry/owner/app:tag). Si se pasa,
     * el stack usa el template rust-image (pull) en vez de compilar. */
    pub image: Option<&'a str>,
    /* [01AA-3] Dónde compila deploy-service: 'laptop' o 'vps'. */
    pub build_mode: &'a str,
    pub skip_theme: bool,
    pub skip_cache: bool,
}

/* Valores efectivos del stack Rust: flags CLI > defaults. */
struct ValoresRust<'a> {
    repo_url: &'a str,
    app_bin: &'a str,
    frontend_dir: &'a str,
    image: Option<&'a str>,
}

/* Ramas de tema y libreria Glory (viajan juntas a cada render de compose). */
struct Ramas<'a> {
    glory_branch: &'a str,
    library_branch: &'a str,
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
    build_mode: BuildMode,
}

pub async fn execute(p: &ParamsNewSite<'_>) -> std::result::Result<(), CoolifyError> {
    let ParamsNewSite {
        config_path,
        site_name,
        domain,
        glory_branch,
        library_branch,
        template,
        target_name,
        repo_url,
        app_bin,
        frontend_dir,
        image,
        build_mode,
        skip_theme,
        skip_cache,
    } = *p;
    /* Validaciones + carga de settings/target + deteccion de placeholder. */
    let (mut settings, target, es_placeholder) =
        cargar_target_y_placeholder(config_path, site_name, domain, image, target_name)?;

    let stack_template = parse_stack_template(template);
    /* [01AA-3] Falla pronto con mensaje claro si el modo no es válido. */
    let build_mode = BuildMode::parse(build_mode)?;

    /* [268A-5] Los stacks que referencian un Dockerfile EXTERNO en disco
     * (Rust: `dockerfile: Dockerfile.rust`; Kamples usa dockerfile_inline pero
     * se trata igual por seguridad) se crean SIN instant_deploy: el Dockerfile
     * aún no está en /data/coolify/services/{uuid} y `docker compose up --build`
     * fallaría al instante. Tras `new` hay que ejecutar `deploy-service`, que
     * sube el Dockerfile, sincroniza el compose y construye con health check.
     * [309A-1/F3] Excepción: Kamples CON --image usa el template por imagen
     * (sin build) y deploy-service no soporta Kamples; se crea CON
     * instant_deploy para que los contenedores arranquen y el Paso 5 instale
     * el tema. Requiere la imagen precargada en la VPS
     * (build-laptop --dockerfile ... antes de `new`). */
    let necesita_deploy_service = matches!(stack_template, StackTemplate::Rust)
        || (matches!(stack_template, StackTemplate::Kamples) && image.is_none());

    tracing::info!("Creando sitio '{site_name}' con dominio {domain} (template: {template})");

    /* [268A-5] Valores efectivos del stack Rust: flags CLI > defaults.
     * Se guardan en settings.json para que el sitio quede correcto desde el
     * primer deploy (antes había que editar settings.json a mano). */
    let (resolved_repo_url, resolved_app_bin, resolved_frontend_dir) =
        resolver_valores_rust(repo_url, app_bin, frontend_dir);

    /* Paso 1: Generar Docker Compose desde template */
    let valores_rust = ValoresRust {
        repo_url: &resolved_repo_url,
        app_bin: &resolved_app_bin,
        frontend_dir: &resolved_frontend_dir,
        image,
    };
    let ramas = Ramas {
        glory_branch,
        library_branch,
    };
    let compose_yaml = generar_compose(
        config_path,
        &settings,
        site_name,
        domain,
        &stack_template,
        &ramas,
        &valores_rust,
    )?;

    /* [119A-5] canonicalize: generar_compose resuelve el template desde
     * templates/ con segmento validado (enum StackTemplate o literal fijo). */

    /* Paso 2: Crear stack en Coolify */
    let api = CoolifyApiClient::new(&target.coolify)?;
    let stack_result = crear_stack_coolify(
        &api,
        site_name,
        &target,
        &compose_yaml,
        necesita_deploy_service,
    )
    .await?;

    registrar_stack_creado(&api, &compose_yaml, &stack_result).await;

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
        build_mode,
    };
    let site_config = construir_site_config(&datos);
    persistir_site_config(&mut settings, config_path, site_config, es_placeholder)?;

    /* Paso 4: Esperar a que los contenedores esten listos + Paso 4.5:
     * reconciliar auth de mariadb ANTES del tema (el WP la necesita para
     * instalarse). Solo Wordpress|Kamples. */
    let es_wordpress = matches!(
        stack_template,
        StackTemplate::Wordpress | StackTemplate::Kamples
    );
    esperar_y_reconciliar_db(&stack_template, &target, &stack_result.uuid, image).await?;

    /* Paso 5: Conectar SSH e instalar tema */
    if !skip_theme && es_wordpress {
        instalar_tema_si_wordpress(
            &settings,
            &target,
            &stack_result.uuid,
            &ramas,
            domain,
            skip_cache,
        )
        .await?;
    }

    mostrar_resumen_creacion(
        site_name,
        domain,
        &target.name,
        &stack_result.uuid,
        necesita_deploy_service,
        image,
    );
    Ok(())
}

/* Traduce el flag --template al enum StackTemplate (119A-6: extraído de execute). */
fn parse_stack_template(template: &str) -> StackTemplate {
    match template {
        "kamples" => StackTemplate::Kamples,
        "minecraft" => StackTemplate::Minecraft,
        "rust" => StackTemplate::Rust,
        _ => StackTemplate::Wordpress,
    }
}

/* Tras create_stack: log + [25A-DB-AUTH] reemplaza STACK_UUID_PLACEHOLDER con el
 * UUID real para evitar colisión DNS en la red compartida coolify.
 * (119A-6: extraído de execute para bajar del límite funcion-larga.) */
async fn registrar_stack_creado(
    api: &CoolifyApiClient,
    compose_yaml: &str,
    stack_result: &crate::domain::StackCreationResult,
) {
    tracing::info!(
        "Stack creado: uuid={}, name={}",
        stack_result.uuid,
        stack_result.name
    );
    /* El template usa postgres-{{STACK_UUID}} en DATABASE_URL y container_name,
     * pero el UUID solo está disponible después de create_stack(). */
    fijar_uuid_en_compose(api, compose_yaml, &stack_result.uuid).await;
}

/* Resumen final de la creación (119A-6: extraído de execute). */
fn mostrar_resumen_creacion(
    site_name: &str,
    domain: &str,
    target_name: &str,
    stack_uuid: &str,
    necesita_deploy_service: bool,
    image: Option<&str>,
) {
    imprimir_resumen(
        site_name,
        domain,
        target_name,
        stack_uuid,
        necesita_deploy_service,
        image,
    );
}

/* [268A-5] Valores efectivos del stack Rust: flags CLI > defaults.
 * (119A-6: extraído de execute para bajar del límite funcion-larga.) */
fn resolver_valores_rust(
    repo_url: Option<&str>,
    app_bin: Option<&str>,
    frontend_dir: Option<&str>,
) -> (String, String, String) {
    let repo = repo_url
        .unwrap_or("https://github.com/1ndoryu/glory-rs.git")
        .to_string();
    let bin = app_bin
        .map(str::to_string)
        .unwrap_or_else(crate::domain::default_app_bin);
    let dir = frontend_dir
        .map(str::to_string)
        .unwrap_or_else(crate::domain::default_frontend_dir);
    (repo, bin, dir)
}

/* Paso 2: crea el stack en Coolify (sin instant_deploy si necesita deploy-service).
 * (119A-6: extraído de execute para bajar del límite funcion-larga.) */
async fn crear_stack_coolify(
    api: &CoolifyApiClient,
    site_name: &str,
    target: &crate::config::DeploymentTargetConfig,
    compose_yaml: &str,
    necesita_deploy_service: bool,
) -> std::result::Result<crate::domain::StackCreationResult, CoolifyError> {
    api.create_stack(
        site_name,
        &target.coolify.server_uuid,
        &target.coolify.project_uuid,
        &target.coolify.environment_name,
        compose_yaml,
        !necesita_deploy_service,
    )
    .await
}

/* Paso 3: persiste el SiteConfig (update si era placeholder, add si es nuevo).
 * (119A-6: extraído de execute para bajar del límite funcion-larga.) */
fn persistir_site_config(
    settings: &mut Settings,
    config_path: &Path,
    site_config: SiteConfig,
    es_placeholder: bool,
) -> std::result::Result<(), CoolifyError> {
    if es_placeholder {
        settings.update_site(site_config, config_path)?;
    } else {
        settings.add_site(site_config, config_path)?;
    }
    Ok(())
}

/* Validaciones + carga de settings/target + deteccion de placeholder. */
fn cargar_target_y_placeholder(
    config_path: &Path,
    site_name: &str,
    domain: &str,
    image: Option<&str>,
    target_name: Option<&str>,
) -> std::result::Result<(Settings, crate::config::DeploymentTargetConfig, bool), CoolifyError> {
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
    let es_placeholder =
        if let Some(existing) = settings.sitios.iter().find(|s| s.nombre == site_name) {
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
async fn fijar_uuid_en_compose(api: &CoolifyApiClient, compose_yaml: &str, stack_uuid: &str) {
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
