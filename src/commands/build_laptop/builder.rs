/* [VPS-89] Modo builder (`build_mode=builder`): la imagen Rust se compila en la
 * VPS de build (`buildHost` de settings.json) y el tgz vuelve al laptop para
 * subirse al target con el mismo envío que el modo laptop. Así ni el target ni
 * el laptop compilan.
 *
 * Relé por el laptop (no VPS→target directo): evita claves SSH entre servidores
 * de producción; el tgz pesa lo mismo en ambos casos. */

use super::pipeline::tarball_path;
use crate::config::VpsConfig;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use std::path::{Path, PathBuf};

const TIMEOUT_BUILD_SECS: u64 = 3600;
const TIMEOUT_SAVE_SECS: u64 = 1800;

/* Compila `tag` en `build_host` con el Dockerfile ya descargado y devuelve el
 * tgz local (en `workdir`). Fail-closed: cualquier fallo remoto aborta antes de
 * tocar el target. Mismos build-args y labels que build_image (laptop). */
pub(super) async fn compilar_en_builder(
    build_host: &VpsConfig,
    tag: &str,
    dockerfile: &Path,
    build_args: &[(String, String)],
    site_name: &str,
    workdir: &Path,
) -> std::result::Result<PathBuf, CoolifyError> {
    let mut ssh = SshClient::from_vps(build_host);
    ssh.connect().await?;
    let docker = ssh
        .execute("docker info --format '{{.ServerVersion}}'")
        .await?;
    if !docker.success() {
        return Err(CoolifyError::Validation(format!(
            "Docker no responde en el build host {}: {}",
            build_host.ip,
            docker.stderr.trim()
        )));
    }

    /* Directorio efímero por sitio: solo Dockerfile, log y tgz. */
    let remote_dir = format!("/tmp/cm-builder/{site_name}");
    let remote_dockerfile = format!("{remote_dir}/Dockerfile.rust");
    let build_log = format!("{remote_dir}/build.log");
    ssh.execute(&format!("mkdir -p {}", sh_quote(&remote_dir)))
        .await?;
    ssh.upload_file_streamed(dockerfile, &remote_dockerfile)
        .await?;

    /* El build corre con nohup + polling: un build Rust tarda minutos sin
     * salida y la sesión SSH podría caerse (ver execute_long_running). */
    let mut args: Vec<String> = vec![
        "docker".into(),
        "build".into(),
        "--pull".into(),
        "--no-cache".into(),
        "--progress=plain".into(),
        "-t".into(),
        sh_quote(tag),
        "--label".into(),
        sh_quote(&format!("cm.local.site={site_name}")),
    ];
    for (clave, valor) in build_args {
        args.push("--build-arg".into());
        args.push(sh_quote(&format!("{clave}={valor}")));
    }
    args.push("-f".into());
    args.push(sh_quote(&remote_dockerfile));
    args.push(sh_quote(&remote_dir));

    println!("Compilando {tag} en la VPS de build ({})...", build_host.ip);
    let build_start = std::time::Instant::now();
    let build = ssh
        .execute_long_running(&args.join(" "), &build_log, 15, TIMEOUT_BUILD_SECS)
        .await?;
    if !build.success() {
        return Err(cola_remota(&mut ssh, "Build", &build_log, build.exit_code).await);
    }
    println!(
        "      Build remoto completado en {}s.",
        build_start.elapsed().as_secs()
    );

    /* `pipefail` obliga a fallar si `docker save` falla: sin él, gzip cerraría
     * un tgz vacío con código 0 y el envío lo tomaría por bueno. */
    let remote_tgz = format!("{remote_dir}/imagen.tgz");
    let save_log = format!("{remote_dir}/save.log");
    let save_inner = format!(
        "docker save {} | gzip > {}",
        sh_quote(tag),
        sh_quote(&remote_tgz)
    );
    super::marca_fase("inicio save | gzip en la VPS de build");
    let save = ssh
        .execute_long_running(
            &format!("bash -o pipefail -c {}", sh_quote(&save_inner)),
            &save_log,
            5,
            TIMEOUT_SAVE_SECS,
        )
        .await?;
    if !save.success() {
        return Err(cola_remota(&mut ssh, "docker save", &save_log, save.exit_code).await);
    }
    let medida = ssh
        .execute(&format!("stat -c %s {}", sh_quote(&remote_tgz)))
        .await?;
    let bytes: u64 = medida.stdout.trim().parse().unwrap_or(0);
    if bytes == 0 {
        return Err(CoolifyError::Validation(format!(
            "El tgz '{remote_tgz}' en la VPS de build está vacío o no existe: '{}'",
            medida.stdout.trim()
        )));
    }

    /* Descarga al laptop: download_file_streamed carga el tgz en memoria, válido
     * para imágenes comprimidas de decenas de MB. */
    let local_tgz = tarball_path(workdir, site_name, tag);
    println!(
        "Descargando imagen desde la VPS de build ({:.1} MB)...",
        bytes as f64 / 1_048_576.0
    );
    super::marca_fase("inicio descarga VPS de build -> laptop");
    ssh.download_file_streamed(&remote_tgz, &local_tgz).await?;
    super::marca_fase("fin descarga VPS de build -> laptop");

    /* Limpieza remota: el tgz y el directorio efímero. Las imágenes taggeadas se
     * conservan como caché; las colgadas (dangling) de builds previos se podan. */
    ssh.execute(&format!("rm -rf {}", sh_quote(&remote_dir)))
        .await?;
    ssh.execute("docker image prune -f >/dev/null 2>&1 || true")
        .await?;
    Ok(local_tgz)
}

/* Error con la cola del log remoto para diagnosticar sin entrar por SSH. */
async fn cola_remota(ssh: &mut SshClient, etapa: &str, log: &str, exit_code: i32) -> CoolifyError {
    let cola = ssh
        .execute(&format!("tail -n 30 {} 2>/dev/null", sh_quote(log)))
        .await
        .map(|out| out.stdout)
        .unwrap_or_else(|_| "(no se pudo leer el log remoto)".into());
    CoolifyError::Validation(format!(
        "{etapa} en la VPS de build falló (exit {exit_code}). Cola:\n{}",
        cola.trim()
    ))
}

/* Comillas simples seguras para `sh -c` (el comando remoto pasa por sh). */
fn sh_quote(valor: &str) -> String {
    format!("'{}'", valor.replace('\'', "'\\''"))
}
