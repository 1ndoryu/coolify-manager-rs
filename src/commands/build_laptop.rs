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
 */

use crate::config::Settings;
use crate::domain::StackTemplate;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, BufReader};

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
 * override vía CM_TMPDIR por si el operador necesita otra unidad. */
fn work_dir(site_name: &str) -> PathBuf {
    let base = std::env::var("CM_TMPDIR").unwrap_or_else(|_| r"C:\tmp".to_string());
    Path::new(&base).join("cm-build").join(site_name)
}

pub async fn execute(params: &ParamsBuildLaptop<'_>) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(params.config_path)?;
    let site = settings.get_site(params.site_name)?;
    validation::assert_site_ready(site)?;

    /* F1 cubre template rust. Kamples necesita su mini-diseño (F3): su
     * Dockerfile es inline y el orden de theme — no reutilizar este camino. */
    if !matches!(site.template, StackTemplate::Rust) {
        return Err(CoolifyError::Validation(format!(
            "build-laptop F1 solo soporta template rust; '{}' usa {:?} (pendiente F3 Kamples)",
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
    let workdir = work_dir(params.site_name);
    tokio::fs::create_dir_all(&workdir).await?;
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
    Ok(())
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
    let workdir = work_dir(&format!("file-{stem}"));
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
    Ok(())
}

/* Preflight laptop compartido (sitio + fichero). */
async fn preflight_local(docker_bin: &str) -> std::result::Result<(), CoolifyError> {
    run_local_checked(docker_bin, &["version"], "docker version").await?;
    run_local_checked(docker_bin, &["system", "df"], "docker system df").await?;
    Ok(())
}

/* Build local con salida en vivo. args/labels ya ordenados por el llamante.
 * Timeout por modo: el stack Kamples compila extensiones PHP desde fuente
 * (pgsql ~8 min) + Node + npm y supera la hora con subida incluida (F4). */
async fn build_image(
    docker_bin: &str,
    tag: &str,
    dockerfile: &Path,
    context: &Path,
    build_args: &[(String, String)],
    labels: &[(String, String)],
    timeout_secs: u64,
) -> std::result::Result<(), CoolifyError> {
    let mut args: Vec<String> = vec![
        "build".into(),
        "--pull".into(),
        "--no-cache".into(),
        "--progress=plain".into(),
        "-t".into(),
        tag.to_string(),
    ];
    for (key, value) in build_args {
        args.push("--build-arg".into());
        args.push(format!("{key}={value}"));
    }
    for (key, value) in labels {
        args.push("--label".into());
        args.push(format!("{key}={value}"));
    }
    args.push("-f".into());
    args.push(dockerfile.display().to_string());
    args.push(context.display().to_string());
    /* [309A-1/F4] Sin secretos aquí (solo tag/dockerfile/contexto): ayuda a
     * diagnosticar cuelgues del CLI (p. ej. rutas verbatim `\\?\`). */
    println!(
        "      Comando: {docker_bin} build -t {tag} -f {} {}",
        dockerfile.display(),
        context.display()
    );
    let build_start = std::time::Instant::now();
    run_local_streaming(docker_bin, &args, timeout_secs).await?;
    println!(
        "      Build local completado en {}s.",
        build_start.elapsed().as_secs()
    );
    Ok(())
}

/* Pasos 2-4 compartidos: save|gzip, guarda de disco, subida, load,
 * inspect fail-closed y limpieza remota (+ local salvo --keep-tarball). */
async fn package_and_ship(
    docker_bin: &str,
    tag: &str,
    workdir: &Path,
    stem: &str,
    ssh: &mut SshClient,
    keep_tarball: bool,
) -> std::result::Result<(), CoolifyError> {
    /* save + gzip en streaming (sin .tar intermedio en disco). */
    let short = tag.rsplit(':').next().unwrap_or("img");
    let tarball = workdir.join(format!("{stem}-{short}.tgz"));
    println!("[2/4] Empaquetando imagen (docker save | gzip)...");
    let tarball_size = save_gzip(docker_bin, tag, &tarball).await?;
    println!(
        "      Tarball: {} ({:.1} MB).",
        tarball.display(),
        tarball_size as f64 / 1_048_576.0
    );

    /* Guarda de disco en VPS: deben caber el .tgz y la imagen descomprimida. */
    let avail_out = ssh
        .execute("df -B1 --output=avail /var/lib/docker | tail -n 1")
        .await?;
    let avail: u64 = avail_out.stdout.trim().parse().map_err(|_| {
        CoolifyError::Validation(format!(
            "No se pudo leer espacio libre en VPS: '{}'",
            avail_out.stdout.trim()
        ))
    })?;
    if avail < tarball_size * 2 {
        return Err(CoolifyError::Validation(format!(
            "Espacio insuficiente en VPS: libres {:.1} MB, necesarios {:.1} MB (tgz + imagen)",
            avail as f64 / 1_048_576.0,
            (tarball_size * 2) as f64 / 1_048_576.0
        )));
    }

    /* Subida + carga + verificación fail-closed + limpieza remota. */
    println!("[3/4] Subiendo al VPS...");
    let remote_tarball = format!("/tmp/cm-laptop-{stem}-{short}.tgz");
    ssh.upload_file_streamed(&tarball, &remote_tarball).await?;
    println!("[4/4] Cargando imagen en el VPS...");
    let load = ssh
        .execute(&format!("gunzip -c '{remote_tarball}' | docker load 2>&1"))
        .await?;
    if !load.success() {
        return Err(CoolifyError::Validation(format!(
            "docker load falló en VPS:\n{}\n{}",
            load.stdout.trim(),
            load.stderr.trim()
        )));
    }
    let inspect = ssh
        .execute(&format!(
            "docker image inspect --format '{{{{.Id}}}}' '{tag}'"
        ))
        .await?;
    if !inspect.success() || inspect.stdout.trim().is_empty() {
        return Err(CoolifyError::Validation(format!(
            "La imagen '{tag}' no quedó cargada en la VPS (inspect vacío). No se continúa."
        )));
    }
    println!("      Imagen cargada: {} ({})", tag, inspect.stdout.trim());
    ssh.execute(&format!("rm -f '{remote_tarball}'")).await?;

    if !keep_tarball {
        tokio::fs::remove_file(&tarball).await?;
    }
    Ok(())
}

/* SHA del BRANCH sin clonar (solo lectura al git remoto). */
async fn resolve_branch_sha(
    docker_bin: &str,
    repo_url: &str,
    branch: &str,
) -> std::result::Result<String, CoolifyError> {
    let _ = docker_bin;
    let out = tokio::process::Command::new("git")
        .args(["ls-remote", repo_url, branch])
        .output()
        .await
        .map_err(|e| {
            CoolifyError::Validation(format!("git ls-remote falló (¿git instalado?): {e}"))
        })?;
    if !out.status.success() {
        return Err(CoolifyError::Validation(format!(
            "git ls-remote de {repo_url} {branch} falló:\n{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let sha = stdout
        .split_whitespace()
        .next()
        .filter(|s| s.len() >= 12 && s.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| {
            CoolifyError::Validation(format!(
                "ls-remote no devolvió SHA para {branch}: '{stdout}'"
            ))
        })?;
    Ok(sha[..12].to_string())
}

/* Ejecuta un comando local corto y exige éxito (preflights). */
async fn run_local_checked(
    bin: &str,
    args: &[&str],
    label: &str,
) -> std::result::Result<String, CoolifyError> {
    let out = tokio::process::Command::new(bin)
        .args(args)
        .output()
        .await
        .map_err(|e| {
            CoolifyError::Validation(format!(
                "{label} falló al lanzar '{bin}' (¿Docker Desktop abierto y PATH recargado?): {e}"
            ))
        })?;
    if !out.status.success() {
        return Err(CoolifyError::Validation(format!(
            "{label} falló:\n{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/* Ejecuta el build mostrando su salida en vivo. Timeout fail-closed con kill.
 * [309A-1/F4] Los DOS pipes se drenan EN PARALELO (una tarea por stream):
 * buildkit vuelca el progreso por stderr y, con un solo lector secuencial
 * (stdout hasta EOF y luego stderr), el hijo bloquea al llenar el buffer
 * de stderr (64 KB) mientras el lector espera un EOF que nunca llega:
 * interbloqueo total sin salida ni CPU (observado en F4). */
async fn run_local_streaming(
    bin: &str,
    args: &[String],
    timeout_secs: u64,
) -> std::result::Result<(), CoolifyError> {
    let mut child = tokio::process::Command::new(bin)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| CoolifyError::Validation(format!("No se pudo lanzar el build local: {e}")))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let reader_out = tokio::spawn(async move {
        let mut tail: Vec<String> = Vec::new();
        if let Some(out) = stdout {
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                println!("{line}");
                push_tail(&mut tail, line);
            }
        }
        tail
    });
    let reader_err = tokio::spawn(async move {
        let mut tail: Vec<String> = Vec::new();
        if let Some(err) = stderr {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("{line}");
                push_tail(&mut tail, line);
            }
        }
        tail
    });
    let status = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), child.wait())
        .await
        .map_err(|_| {
            CoolifyError::Validation(format!("Build local superó {timeout_secs}s; abortado"))
        })?
        .map_err(|e| CoolifyError::Validation(format!("Espera del build local falló: {e}")))?;
    let (tail_out, tail_err) = tokio::join!(reader_out, reader_err);
    /* Cola combinada (orden entre streams no cronológico, vale para
     * diagnosticar el fallo sin pretender ser un log fiel). */
    let mut tail = tail_out.unwrap_or_default();
    tail.extend(tail_err.unwrap_or_default());
    let tail: Vec<String> = tail.into_iter().rev().take(30).rev().collect();
    if !status.success() {
        return Err(CoolifyError::Validation(format!(
            "Build local falló (exit {}). Cola:\n{}",
            status.code().unwrap_or(-1),
            tail.join("\n")
        )));
    }
    Ok(())
}

fn push_tail(tail: &mut Vec<String>, line: String) {
    tail.push(line);
    if tail.len() > 30 {
        tail.remove(0);
    }
}

/* `docker save <tag> | gzip > tarball` en streaming dentro de un hilo
 * bloqueante (evita cargar la imagen en RAM y no deja .tar en disco). */
async fn save_gzip(
    docker_bin: &str,
    tag: &str,
    tarball: &Path,
) -> std::result::Result<u64, CoolifyError> {
    let bin = docker_bin.to_string();
    let tag = tag.to_string();
    let tarball = tarball.to_path_buf();
    tokio::task::spawn_blocking(move || -> std::result::Result<u64, CoolifyError> {
        let mut child = std::process::Command::new(&bin)
            .args(["save", &tag])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| CoolifyError::Validation(format!("docker save falló al lanzar: {e}")))?;
        let pipe = child.stdout.take().ok_or_else(|| {
            CoolifyError::Validation("docker save sin stdout capturable".to_string())
        })?;
        let file = std::fs::File::create(&tarball).map_err(|e| {
            CoolifyError::Validation(format!("No se pudo crear {}: {e}", tarball.display()))
        })?;
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut reader = std::io::BufReader::with_capacity(131_072, pipe);
        let mut writer = std::io::BufWriter::with_capacity(131_072, encoder);
        std::io::copy(&mut reader, &mut writer)
            .map_err(|e| CoolifyError::Validation(format!("Compresión del tarball falló: {e}")))?;
        let mut encoder = writer
            .into_inner()
            .map_err(|e| CoolifyError::Validation(format!("Cierre del buffer gzip falló: {e}")))?;
        encoder
            .try_finish()
            .map_err(|e| CoolifyError::Validation(format!("Finalización del gzip falló: {e}")))?;
        let status = child
            .wait()
            .map_err(|e| CoolifyError::Validation(format!("Espera de docker save falló: {e}")))?;
        if !status.success() {
            return Err(CoolifyError::Validation(
                "docker save terminó con error".to_string(),
            ));
        }
        std::fs::metadata(&tarball)
            .map(|m| m.len())
            .map_err(|e| CoolifyError::Validation(format!("No se pudo medir el tarball: {e}")))
    })
    .await
    .map_err(|e| CoolifyError::Validation(format!("Tarea save/gzip falló: {e}")))?
}
