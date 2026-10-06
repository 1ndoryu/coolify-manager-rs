/* Pipeline local: build, empaquetado, subida y carga [06AA-4 split build_laptop]. */

use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};

/* Build local con salida en vivo. args/labels ya ordenados por el llamante.
 * Timeout por modo: el stack Kamples compila extensiones PHP desde fuente
 * (pgsql ~8 min) + Node + npm y supera la hora con subida incluida (F4). */
pub(super) async fn build_image(
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
pub(super) async fn package_and_ship(
    docker_bin: &str,
    tag: &str,
    workdir: &Path,
    stem: &str,
    ssh: &mut SshClient,
    keep_tarball: bool,
) -> std::result::Result<(), CoolifyError> {
    /* save + gzip en streaming (sin .tar intermedio en disco).
     * [06AA-3] canonicalize: stem saneado (solo alfanumérico/-/_, no vacío)
     * y short deriva del tag generado internamente (formato fijo
     * `{site}:cm-...`), nunca input crudo; workdir ya validado por
     * work_dir(). Sin vector de traversal. */
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
pub(super) async fn resolve_branch_sha(
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
pub(super) async fn run_local_checked(
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
