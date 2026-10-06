/* [309A-1/F1] Comando: build-laptop
 * Compila la imagen Docker en el laptop (no en el VPS), la comprime, la sube
 * por SSH y la carga en el daemon del VPS con `docker load`.
 *
 * Motivación: el build Rust (~10 min al 100% CPU) provocó el reinicio de
 * dockerd del 2026-09-20 con 11 sitios caídos. Compilar fuera elimina ese
 * riesgo sin depender de GitHub Actions ni de un registry externo.
 *
 * Equivalencia con el build en VPS: se descarga el Dockerfile on-disk del
 * servicio (`/data/coolify/services/<uuid>/Dockerfile.rust`, el mismo que
 * usa `docker compose build` en la VPS) y se compila con los mismos
 * build-args (REPO_URL/BRANCH/APP_BIN/FRONTEND_DIR + VITE_* de Coolify).
 * El Dockerfile clona el repo dentro del builder, así que el contexto local
 * solo necesita el propio Dockerfile.
 *
 * Tag: `cm-local/<sitio>:<sha12>` (sha del BRANCH vía `git ls-remote`),
 * nunca `latest`. El consumo (deploy-service --image) llega en F2.
 *
 * [309A-1/F3] Modo fichero (--dockerfile + --tag): para Dockerfiles SIN
 * sitio previo (Kamples: Dockerfile.kamples en config/templates). Sin vars
 * de Coolify (el stack PHP no tiene build-args) y con tag explícito
 * (Kamples no tiene repo/branch del que derivar sha). Subida/carga contra
 * el target indicado (default si se omite).
 *
 * [06AA-4] Split por dominio: `preflight` (daemon + disco), `rotacion`
 * (poda/rotación de tags cm-local) y `pipeline` (build/empaquetado/subida).
 * Superficie pública intacta (execute/execute_file/Params + rotación
 * pub(crate) re-exportada para deploy-service).
 */

mod pipeline;
mod preflight;
mod rotacion;

use pipeline::{build_image, package_and_ship, resolve_branch_sha};
use preflight::preflight_local;
use rotacion::etiqueta_kamples;

pub(crate) use rotacion::{podar_colgadas, rotar_etiquetas_laptop, rotar_etiquetas_vps};

use crate::config::Settings;
use crate::domain::StackTemplate;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use std::path::{Path, PathBuf};

pub struct ParamsBuildLaptop<'a> {
    pub config_path: &'a Path,
    pub site_name: &'a str,
    pub tag: Option<&'a str>,
    pub keep_tarball: bool,
    pub docker_bin: &'a str,
}

/* [309A-1/F3] Parámetros del modo fichero: Dockerfile suelto + tag explícito. */
pub struct ParamsBuildLaptopFile<'a> {
    pub config_path: &'a Path,
    pub dockerfile: &'a str,
    pub tag: &'a str,
    pub target: Option<&'a str>,
    pub keep_tarball: bool,
    pub docker_bin: &'a str,
}

/* Directorio temporal de artefactos. Respeta la regla del área (C:\tmp) con
 * override vía CM_TMPDIR por si el operador necesita otra unidad.
 * [06AA-3] canonicalize: site_name viene del CLI; join_segmento_seguro
 * valida el segmento (sin `..`/separadores) y verifica starts_with bajo
 * la base cuando el resultado existe en disco. */
fn work_dir(site_name: &str) -> std::result::Result<PathBuf, CoolifyError> {
    let base = std::env::var("CM_TMPDIR").unwrap_or_else(|_| r"C:\tmp".to_string());
    let base = Path::new(&base).join("cm-build");
    validation::join_segmento_seguro(&base, site_name, "site")
}

/* [01AA-3/F2] Devuelve el tag construido (deploy-service lo usa como
 * override en memoria sin tocar settings.json). */
pub async fn execute(params: &ParamsBuildLaptop<'_>) -> std::result::Result<String, CoolifyError> {
    let settings = Settings::load(params.config_path)?;
    let site = settings.get_site(params.site_name)?;
    validation::assert_site_ready(site)?;

    /* [01AA-3/F2] Kamples compila por modo fichero con tag automático
     * (no tiene repo/branch del que derivar sha). Otros templates sin
     * build local siguen rechazados con mensaje claro. */
    if matches!(site.template, StackTemplate::Kamples) {
        return execute_kamples(params, site).await;
    }
    if !matches!(site.template, StackTemplate::Rust) {
        return Err(CoolifyError::Validation(format!(
            "build-laptop solo soporta templates rust/kamples; '{}' usa {:?}",
            params.site_name, site.template
        )));
    }
    let stack_uuid = site.stack_uuid.as_deref().ok_or_else(|| {
        CoolifyError::Validation(format!(
            "Sitio '{}' sin stackUuid configurado",
            params.site_name
        ))
    })?;
    let target = settings.resolve_site_target(site)?;
    let service_dir = format!("/data/coolify/services/{stack_uuid}");

    let repo_url = site
        .repo_url
        .as_deref()
        .unwrap_or("https://github.com/1ndoryu/glory-rs.git");

    /* Tag fijo: override validado o cm-local/<sitio>:<sha12> del BRANCH. */
    let tag = match params.tag {
        Some(custom) => custom.to_string(),
        None => {
            let sha = resolve_branch_sha(params.docker_bin, repo_url, &site.glory_branch).await?;
            format!("cm-local/{}:{sha}", params.site_name.to_lowercase())
        }
    };
    validation::validate_image_ref(&tag)?;

    /* Preflight laptop: el daemon debe responder antes de compilar. */
    preflight_local(params.docker_bin).await?;

    /* Envs de build-time VITE_* desde Coolify (los mismos del build en VPS). */
    let vite_pairs =
        super::deploy_service::env_building::vite_build_pairs(&target.coolify, stack_uuid).await?;
    println!("      Build envs Vite desde Coolify: {}", vite_pairs.len());

    /* Dockerfile on-disk del servicio: la fuente exacta del build en VPS. */
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;
    let remote_dockerfile = format!("{service_dir}/Dockerfile.rust");
    let check = ssh
        .execute(&format!("test -f '{remote_dockerfile}'"))
        .await?;
    if !check.success() {
        return Err(CoolifyError::Validation(format!(
            "Sin Dockerfile on-disk en '{remote_dockerfile}': el stack no es rust-externo o la creación no subió el Dockerfile"
        )));
    }
    let workdir = work_dir(params.site_name)?;
    tokio::fs::create_dir_all(&workdir).await?;
    /* [06AA-3] canonicalize: "Dockerfile.rust" es literal fijo del
     * comando (no input externo); el join directo es seguro. */
    let local_dockerfile = workdir.join("Dockerfile.rust");
    ssh.download_file(&remote_dockerfile, &local_dockerfile)
        .await?;
    println!("      Dockerfile on-disk descargado.");

    /* Build local con los mismos args que fases_nucleo::fase_build. */
    let mut build_args: Vec<(String, String)> = vec![
        ("REPO_URL".to_string(), repo_url.to_string()),
        ("BRANCH".to_string(), site.glory_branch.clone()),
        ("APP_BIN".to_string(), site.app_bin.clone()),
        ("FRONTEND_DIR".to_string(), site.frontend_dir.clone()),
    ];
    build_args.extend(vite_pairs);
    println!("[1/4] Compilando {tag} en el laptop (varios minutos, sin tocar el VPS)...");
    build_image(
        params.docker_bin,
        &tag,
        &local_dockerfile,
        &workdir,
        &build_args,
        &[("cm.local.site".to_string(), params.site_name.to_string())],
        3600,
    )
    .await?;

    /* save + gzip + subida + carga + verificación + limpieza (pasos 2-4). */
    package_and_ship(
        params.docker_bin,
        &tag,
        &workdir,
        params.site_name,
        &mut ssh,
        params.keep_tarball,
    )
    .await?;

    println!("\nOK: '{tag}' compilada en laptop y cargada en el VPS sin build remoto.");
    /* [01AA-3/F2c] Limpieza automática: solo colgadas (tags intactos). */
    podar_colgadas(params.docker_bin).await;
    Ok(tag)
}

/* [01AA-3/F2] Sitio Kamples: Dockerfile.kamples de config/templates + tag
 * automático fecha-hora (con segundos: evita colisiones entre builds). */
async fn execute_kamples(
    params: &ParamsBuildLaptop<'_>,
    site: &crate::domain::SiteConfig,
) -> std::result::Result<String, CoolifyError> {
    let tag = match params.tag {
        Some(custom) => custom.to_string(),
        None => {
            let marca = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
            etiqueta_kamples(params.site_name, &marca)
        }
    };
    validation::validate_image_ref(&tag)?;
    let dockerfile = params
        .config_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("templates")
        .join("Dockerfile.kamples");
    let dockerfile_str = dockerfile.to_string_lossy().into_owned();
    execute_file(&ParamsBuildLaptopFile {
        config_path: params.config_path,
        dockerfile: &dockerfile_str,
        tag: &tag,
        target: site.target.as_deref(),
        keep_tarball: params.keep_tarball,
        docker_bin: params.docker_bin,
    })
    .await?;
    Ok(tag)
}

/* [309A-1/F3] Modo fichero: compila un Dockerfile suelto (sin sitio previo),
 * lo transfiere y lo carga en la VPS. Camino Kamples: Dockerfile.kamples en
 * config/templates + `new --template kamples --image <tag>`. */
pub async fn execute_file(
    params: &ParamsBuildLaptopFile<'_>,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(params.config_path)?;
    let target = match params.target {
        Some(name) => settings.get_target(name)?.clone(),
        None => settings.default_target(),
    };
    validation::validate_image_ref(params.tag)?;

    /* Dockerfile local: debe existir y ser un fichero (fail-closed).
     * canonicalize resuelve `..`/enlaces, pero en Windows devuelve ruta
     * verbatim (`\\?\C:\...`) y el CLI docker se queda colgado con ella
     * (build sin salida, observado en F4): se despoja el prefijo. */
    let dockerfile = PathBuf::from(params.dockerfile)
        .canonicalize()
        .map(|p| {
            let s = p.display().to_string();
            PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string())
        })
        .map_err(|e| {
            CoolifyError::Validation(format!(
                "Dockerfile '{}' no legible: {e}",
                params.dockerfile
            ))
        })?;
    if !dockerfile.is_file() {
        return Err(CoolifyError::Validation(format!(
            "Dockerfile '{}' no es un fichero",
            dockerfile.display()
        )));
    }
    let context = dockerfile.parent().unwrap_or(Path::new(".")).to_path_buf();
    /* Stem saneado para nombrar workdir/tarball (sin ../ ni espacios). */
    let stem: String = dockerfile
        .file_stem()
        .map(|s| {
            s.to_string_lossy()
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect()
        })
        .filter(|s: &String| !s.is_empty())
        .unwrap_or_else(|| "file".to_string());

    preflight_local(params.docker_bin).await?;
    /* [06AA-3] canonicalize: stem saneado (solo alfanumérico/-/_); el join
     * con prefijo fijo "file-" no admite traversal. */
    let workdir = work_dir(&format!("file-{stem}"))?;
    tokio::fs::create_dir_all(&workdir).await?;

    /* Sin build-args: el stack PHP (Kamples) no consume vars de Coolify. */
    println!(
        "[1/4] Compilando {} en el laptop (varios minutos, sin tocar el VPS)...",
        params.tag
    );
    build_image(
        params.docker_bin,
        params.tag,
        &dockerfile,
        &context,
        &[],
        &[("cm.local.file".to_string(), stem.clone())],
        5400,
    )
    .await?;

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;
    package_and_ship(
        params.docker_bin,
        params.tag,
        &workdir,
        &stem,
        &mut ssh,
        params.keep_tarball,
    )
    .await?;

    println!(
        "\nOK: '{}' compilada en laptop y cargada en el VPS sin build remoto.",
        params.tag
    );
    /* [01AA-3/F2c] Limpieza automática: solo colgadas (tags intactos). */
    podar_colgadas(params.docker_bin).await;
    Ok(())
}
