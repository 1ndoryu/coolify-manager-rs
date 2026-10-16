/* [VPS-89] Modo builder (`build_mode=builder`): la imagen Rust se compila en la
 * VPS de build (`buildHost` de settings.json). Desde ahí llega al target por una
 * de dos rutas (`buildTransfer`):
 *  - vps (defecto): relevo directo 89→target. El laptop solo reenvía su agente
 *    SSH (`ssh -A`) para que la 89 se autentique en el target. Ninguna clave se
 *    copia a un servidor y el tgz no pasa por el laptop.
 *  - laptop: descarga 89→laptop y subida laptop→target (sin agente reenviado).
 *
 * [14AA-1/F4] Medido: el tramo 89→laptop era ~58 % del tiempo de un build-only. */

use super::contexto::{ContextoLocal, BUNDLE_APP, BUNDLE_FRAMEWORK};
use super::pipeline::tarball_path;
use crate::config::VpsConfig;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const TIMEOUT_BUILD_SECS: u64 = 3600;
const TIMEOUT_SAVE_SECS: u64 = 1800;
/* ~60 MB a ~4 MB/s son ~15 s; el margen cubre redes lentas sin colgar el deploy. */
const TIMEOUT_RELEVO_SECS: u64 = 1800;
/* Git for Windows: el ssh-agent y el ssh de MSYS son los que comparten el agente
 * arrancado en la misma invocación. El agente de System32 requiere servicio (admin). */
const GIT_BASH_DEFECTO: &str = r"C:\Program Files\Git\usr\bin\bash.exe";

/* Imagen guardada en el build host (`docker save | gzip`), lista para bajar o relevar. */
pub(super) struct ImagenRemota {
    pub(super) remote_dir: String,
    pub(super) remote_tgz: String,
    pub(super) bytes: u64,
    pub(super) sha256: String,
}

/* Compila `tag` en `build_host` con el Dockerfile ya descargado y guarda el tgz
 * en el build host. Fail-closed: cualquier fallo remoto aborta antes de tocar el
 * target. Mismos build-args y labels que build_image (laptop).
 * [14AA-1/F4] Con `contexto` (repoLocal), el contexto de build son los bundles del
 * laptop; sin él, el Dockerfile clona de GitHub dentro del build. */
/* Rutas del directorio de build en la VPS de build (`~/.cm-builder/<sitio>`). */
struct SitioRemoto {
    dir: String,
    dockerfile: String,
    log_build: String,
    tgz: String,
}

pub(super) async fn compilar_en_builder(
    build_host: &VpsConfig,
    tag: &str,
    dockerfile: &Path,
    build_args: &[(String, String)],
    site_name: &str,
    contexto: Option<&ContextoLocal>,
) -> std::result::Result<ImagenRemota, CoolifyError> {
    let mut ssh = SshClient::from_vps(build_host);
    ssh.connect().await?;
    comprobar_docker(&mut ssh, build_host).await?;
    let sitio = preparar_sitio_remoto(&mut ssh, site_name, dockerfile, contexto).await?;
    lanzar_build_remoto(&mut ssh, build_host, tag, site_name, build_args, &sitio).await?;
    let (bytes, sha256) = guardar_tgz_remoto(&mut ssh, tag, &sitio).await?;
    Ok(ImagenRemota {
        remote_dir: sitio.dir,
        remote_tgz: sitio.tgz,
        bytes,
        sha256,
    })
}

/* Fail-closed antes de tocar el target: sin daemon no hay build. */
async fn comprobar_docker(
    ssh: &mut SshClient,
    build_host: &VpsConfig,
) -> std::result::Result<(), CoolifyError> {
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
    Ok(())
}

/* Deja en el build host el directorio del sitio con el Dockerfile y, si hay
 * contexto local, los bundles. */
async fn preparar_sitio_remoto(
    ssh: &mut SshClient,
    site_name: &str,
    dockerfile: &Path,
    contexto: Option<&ContextoLocal>,
) -> std::result::Result<SitioRemoto, CoolifyError> {
    /* Directorio efímero por sitio bajo el HOME del usuario de build: en /tmp puede
     * existir un directorio de root (mkdir falla para el usuario de build). */
    let home = ssh
        .execute("printf '%s' \"$HOME\"")
        .await?
        .stdout
        .trim()
        .to_string();
    if !home.starts_with('/') {
        return Err(CoolifyError::Validation(format!(
            "No se pudo leer $HOME en la VPS de build: '{home}'"
        )));
    }
    let dir = format!("{home}/.cm-builder/{site_name}");
    let sitio = SitioRemoto {
        dockerfile: format!("{dir}/Dockerfile.rust"),
        log_build: format!("{dir}/build.log"),
        tgz: format!("{dir}/imagen.tgz"),
        dir,
    };
    ssh.execute(&format!("mkdir -p {}", sh_quote(&sitio.dir)))
        .await?;
    /* Restos de una ejecución anterior: el contexto de build es el directorio
     * entero, así que un tgz viejo (decenas de MB) viajaría en cada build. */
    ssh.execute(&format!(
        "rm -f {} {} {}",
        sh_quote(&sitio.tgz),
        sh_quote(&format!("{}/{BUNDLE_APP}", sitio.dir)),
        sh_quote(&format!("{}/{BUNDLE_FRAMEWORK}", sitio.dir)),
    ))
    .await?;
    ssh.upload_file_streamed(dockerfile, &sitio.dockerfile)
        .await?;
    if let Some(ctx) = contexto {
        ssh.upload_file_streamed(&ctx.app_bundle, &format!("{}/{BUNDLE_APP}", sitio.dir))
            .await?;
        ssh.upload_file_streamed(
            &ctx.framework_bundle,
            &format!("{}/{BUNDLE_FRAMEWORK}", sitio.dir),
        )
        .await?;
    }
    Ok(sitio)
}

/* `docker build` con nohup + polling: un build Rust tarda minutos sin salida y la
 * sesión SSH podría caerse (ver execute_long_running). Sin --no-cache: CACHE_BUST
 * (sha del commit, en build_args) invalida el clone y las capas posteriores solo
 * cuando cambia el commit; lo demás se reutiliza. */
async fn lanzar_build_remoto(
    ssh: &mut SshClient,
    build_host: &VpsConfig,
    tag: &str,
    site_name: &str,
    build_args: &[(String, String)],
    sitio: &SitioRemoto,
) -> std::result::Result<(), CoolifyError> {
    let mut args: Vec<String> = vec![
        "docker".into(),
        "build".into(),
        "--pull".into(),
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
    args.push(sh_quote(&sitio.dockerfile));
    args.push(sh_quote(&sitio.dir));

    println!("Compilando {tag} en la VPS de build ({})...", build_host.ip);
    let build_start = std::time::Instant::now();
    let build = ssh
        .execute_long_running(&args.join(" "), &sitio.log_build, 5, TIMEOUT_BUILD_SECS)
        .await?;
    if !build.success() {
        return Err(cola_remota(ssh, "Build", &sitio.log_build, build.exit_code).await);
    }
    println!(
        "      Build remoto completado en {}s.",
        build_start.elapsed().as_secs()
    );
    Ok(())
}

/* `docker save | gzip` en el build host. Devuelve (bytes, sha256): el target
 * vuelve a comprobar el sha antes de cargar. */
async fn guardar_tgz_remoto(
    ssh: &mut SshClient,
    tag: &str,
    sitio: &SitioRemoto,
) -> std::result::Result<(u64, String), CoolifyError> {
    /* `pipefail` obliga a fallar si `docker save` falla: sin él, gzip cerraría
     * un tgz vacío con código 0 y el envío lo tomaría por bueno. */
    let save_log = format!("{}/save.log", sitio.dir);
    let save_inner = format!(
        "docker save {} | gzip > {}",
        sh_quote(tag),
        sh_quote(&sitio.tgz)
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
        return Err(cola_remota(ssh, "docker save", &save_log, save.exit_code).await);
    }
    let medida = ssh
        .execute(&format!("stat -c %s {}", sh_quote(&sitio.tgz)))
        .await?;
    let bytes: u64 = medida.stdout.trim().parse().unwrap_or(0);
    if bytes == 0 {
        return Err(CoolifyError::Validation(format!(
            "El tgz '{}' en la VPS de build está vacío o no existe: '{}'",
            sitio.tgz,
            medida.stdout.trim()
        )));
    }
    let sha = ssh
        .execute(&format!("sha256sum {} | cut -c1-64", sh_quote(&sitio.tgz)))
        .await?;
    let sha256 = sha.stdout.trim().to_string();
    if !sha.success() || sha256.len() != 64 {
        return Err(CoolifyError::Validation(format!(
            "No se pudo calcular el sha256 de '{}' en la VPS de build: '{}'",
            sitio.tgz,
            sha.stdout.trim()
        )));
    }
    Ok((bytes, sha256))
}

/* Ruta `buildTransfer=laptop`: descarga el tgz al laptop y limpia el build host.
 * download_file_streamed carga el tgz en memoria, válido para decenas de MB. */
pub(super) async fn descargar_a_laptop(
    build_host: &VpsConfig,
    imagen: &ImagenRemota,
    site_name: &str,
    tag: &str,
    workdir: &Path,
) -> std::result::Result<PathBuf, CoolifyError> {
    let mut ssh = SshClient::from_vps(build_host);
    ssh.connect().await?;
    let local_tgz = tarball_path(workdir, site_name, tag)?;
    println!(
        "Descargando imagen desde la VPS de build ({:.1} MB)...",
        imagen.bytes as f64 / 1_048_576.0
    );
    super::marca_fase("inicio descarga VPS de build -> laptop");
    ssh.download_file_streamed(&imagen.remote_tgz, &local_tgz)
        .await?;
    super::marca_fase("fin descarga VPS de build -> laptop");
    limpiar_build_host(&mut ssh, imagen).await?;
    Ok(local_tgz)
}

/* Ruta `buildTransfer=vps` (defecto): relevo directo build host → target.
 * El laptop arranca un ssh-agent temporal con sus claves, entra a la 89 con
 * `ssh -A` y pide allí un `scp` hacia el target: la 89 se autentica en el target
 * con el agente reenviado. El tgz viaja 89→target; el laptop solo transporta órdenes.
 * Garantías: known_hosts temporal en la 89 con la clave pública del target (la del
 * known_hosts del laptop; sin ella el scp no conecta), sha256 comprobado en la 89
 * antes de enviar, y limpieza de la 89 pase lo que pase (trap en el script remoto).
 * Devuelve la ruta del tgz en el target; el target verifica el sha antes de cargar. */
pub(super) async fn relevar_a_target(
    build_host: &VpsConfig,
    target: &VpsConfig,
    imagen: &ImagenRemota,
) -> std::result::Result<String, CoolifyError> {
    for vps in [build_host, target] {
        if vps.ssh_password.is_some() {
            return Err(CoolifyError::Validation(
                "buildTransfer=vps requiere clave SSH (ssh -A no admite sshPassword); usa buildTransfer=laptop".into(),
            ));
        }
    }
    let clave_build = clave_laptop(build_host)?;
    let mut claves = vec![clave_build.clone()];
    let clave_target = clave_laptop(target)?;
    if !claves.contains(&clave_target) {
        claves.push(clave_target);
    }
    let kh_laptop = std::fs::read_to_string(known_hosts_laptop()?).map_err(|e| {
        CoolifyError::Validation(format!("No se pudo leer known_hosts del laptop: {e}"))
    })?;
    let lineas_kh = lineas_known_hosts(&kh_laptop, &target.ip);
    if lineas_kh.is_empty() {
        return Err(CoolifyError::Validation(format!(
            "known_hosts del laptop sin entrada para {} (conéctate una vez por ssh o usa buildTransfer=laptop)",
            target.ip
        )));
    }

    let destino_remoto = format!("/tmp/cm-relay-{}.tgz", &imagen.sha256[..16]);
    let destino_usuario_ip = format!("{}@{}", target.user, target.ip);
    let script_89 = script_relevo_89(imagen, &destino_usuario_ip, &lineas_kh, &destino_remoto);
    let script_laptop = script_laptop(build_host, &clave_build, &claves);
    let bash = git_bash()?;

    let mut child = tokio::process::Command::new(&bash)
        .arg("-c")
        .arg(&script_laptop)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            CoolifyError::Validation(format!(
                "No se pudo lanzar Git Bash ({}): {e}",
                bash.display()
            ))
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        /* El script de la 89 entra por stdin: no hay valores en la línea de comandos. */
        stdin.write_all(script_89.as_bytes()).await?;
        stdin.shutdown().await?;
    }

    println!(
        "Relevo directo {} -> {} ({:.1} MB)...",
        build_host.ip,
        target.ip,
        imagen.bytes as f64 / 1_048_576.0
    );
    super::marca_fase("inicio relevo VPS de build -> target");
    let salida = tokio::time::timeout(
        Duration::from_secs(TIMEOUT_RELEVO_SECS),
        child.wait_with_output(),
    )
    .await
    .map_err(|_| {
        CoolifyError::Validation(format!(
            "El relevo 89→target superó {TIMEOUT_RELEVO_SECS} s y se cancela"
        ))
    })??;
    super::marca_fase("fin relevo VPS de build -> target");
    if !salida.status.success() {
        return Err(CoolifyError::Validation(format!(
            "Relevo 89→target falló (exit {}):\n{}",
            salida.status.code().unwrap_or(-1),
            ultimas_lineas(&String::from_utf8_lossy(&salida.stderr))
        )));
    }
    Ok(destino_remoto)
}

/* Limpieza del build host tras descargar: tgz y directorio efímero. Las imágenes
 * taggeadas se conservan como caché; las colgadas (dangling) se podan. */
async fn limpiar_build_host(
    ssh: &mut SshClient,
    imagen: &ImagenRemota,
) -> std::result::Result<(), CoolifyError> {
    ssh.execute(&format!("rm -rf {}", sh_quote(&imagen.remote_dir)))
        .await?;
    ssh.execute("docker image prune -f >/dev/null 2>&1 || true")
        .await?;
    Ok(())
}

/* Script de la 89 (llega por stdin a `bash -s`). Verifica el sha256 antes de
 * enviar. El trap borra el known_hosts temporal, el directorio efímero y las
 * colgadas, también si el scp falla. */
fn script_relevo_89(
    imagen: &ImagenRemota,
    destino_usuario_ip: &str,
    lineas_kh: &[String],
    destino_remoto: &str,
) -> String {
    let mut s = String::from("set -u\numask 077\nKH=\"$(mktemp)\"\n");
    s.push_str(&format!("DIR={}\n", sh_quote(&imagen.remote_dir)));
    s.push_str(
        r#"trap 'rm -f "$KH"; rm -rf "$DIR"; docker image prune -f >/dev/null 2>&1 || true' EXIT
"#,
    );
    let lineas: Vec<String> = lineas_kh.iter().map(|l| sh_quote(l)).collect();
    s.push_str(&format!("printf '%s\\n' {} > \"$KH\"\n", lineas.join(" ")));
    s.push_str(&format!("TGZ={}\n", sh_quote(&imagen.remote_tgz)));
    s.push_str("ACTUAL=\"$(sha256sum \"$TGZ\" | cut -c1-64)\"\n");
    s.push_str(&format!(
        "[ \"$ACTUAL\" = {} ] || {{ echo \"sha256 en build host no coincide: $ACTUAL\" >&2; exit 20; }}\n",
        sh_quote(&imagen.sha256)
    ));
    s.push_str(&format!(
        "scp -q -o BatchMode=yes -o StrictHostKeyChecking=yes -o UserKnownHostsFile=\"$KH\" -o ConnectTimeout=30 \"$TGZ\" {}:{}\n",
        sh_quote(destino_usuario_ip),
        sh_quote(destino_remoto)
    ));
    s
}

/* Script del laptop (Git Bash): agente temporal con las claves, `ssh -A` a la 89 y
 * el script de la 89 por stdin. El agente se mata al salir (trap). Sin passphrase
 * (ssh-add no puede pedirla sin terminal): el fallo se reporta, no se reintenta. */
fn script_laptop(build_host: &VpsConfig, clave_build: &Path, claves: &[PathBuf]) -> String {
    let mut s = String::from("set -u\n");
    s.push_str(
        "eval \"$(ssh-agent -s)\" >/dev/null || { echo 'ssh-agent no arranca' >&2; exit 10; }\n",
    );
    s.push_str("trap 'ssh-agent -k >/dev/null 2>&1' EXIT\n");
    for clave in claves {
        let ruta = ruta_bash(clave);
        s.push_str(&format!(
            "ssh-add {} >/dev/null 2>&1 || {{ echo 'ssh-add fallo con {}: clave inexistente o con passphrase' >&2; exit 11; }}\n",
            sh_quote(&ruta),
            ruta
        ));
    }
    s.push_str(&format!(
        "ssh -A -i {} -o IdentitiesOnly=yes -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=30 -o ServerAliveInterval=30 {} 'bash -s'\n",
        sh_quote(&ruta_bash(clave_build)),
        sh_quote(&format!("{}@{}", build_host.user, build_host.ip))
    ));
    s.push_str("exit $?\n");
    s
}

/* Líneas de known_hosts que corresponden a `ip` (plano o `[ip]:puerto`). Los hosts
 * hasheados (`|1|...`) no se pueden casar y se ignoran. */
fn lineas_known_hosts(contenido: &str, ip: &str) -> Vec<String> {
    let prefijo_puerto = format!("[{ip}]:");
    contenido
        .lines()
        .filter(|linea| {
            let linea = linea.trim();
            let host = linea.split_whitespace().next().unwrap_or("");
            !linea.is_empty()
                && !linea.starts_with('#')
                && !host.starts_with('|')
                && host
                    .split(',')
                    .any(|h| h == ip || h.starts_with(&prefijo_puerto))
        })
        .map(|linea| linea.trim().to_string())
        .collect()
}

/* Misma resolución de clave que SshClient: `sshKey` o ~/.ssh/id_ed25519 / id_rsa. */
fn clave_laptop(vps: &VpsConfig) -> std::result::Result<PathBuf, CoolifyError> {
    if let Some(ruta) = vps.ssh_key.as_deref() {
        return Ok(PathBuf::from(ruta));
    }
    let ssh_dir = dirs::home_dir()
        .ok_or_else(|| {
            CoolifyError::Validation("Sin carpeta home para buscar la clave SSH".into())
        })?
        .join(".ssh");
    for nombre in ["id_ed25519", "id_rsa"] {
        let ruta = ssh_dir.join(nombre);
        if ruta.is_file() {
            /* canonicalize solo para la comprobación: la ruta devuelta va a
             * `ssh -i` y en Windows la forma verbatim (`\\?\C:\...`) es la que
             * deja colgado al cliente docker (ver execute_file). */
            if let (Ok(dir), Ok(destino)) = (ssh_dir.canonicalize(), ruta.canonicalize()) {
                if !destino.starts_with(&dir) {
                    return Err(CoolifyError::Validation(format!(
                        "La clave SSH '{}' escapa de {}",
                        ruta.display(),
                        ssh_dir.display()
                    )));
                }
            }
            return Ok(ruta);
        }
    }
    Err(CoolifyError::Validation(format!(
        "No hay clave SSH para {} (sshKey en settings o {}/id_ed25519)",
        vps.ip,
        ssh_dir.display()
    )))
}

fn known_hosts_laptop() -> std::result::Result<PathBuf, CoolifyError> {
    dirs::home_dir()
        .map(|home| home.join(".ssh").join("known_hosts"))
        .ok_or_else(|| CoolifyError::Validation("Sin carpeta home para known_hosts".into()))
}

/* Git Bash del laptop; `CM_GIT_BASH` lo sobrescribe si Git no está en la ruta por defecto. */
fn git_bash() -> std::result::Result<PathBuf, CoolifyError> {
    let ruta = std::env::var_os("CM_GIT_BASH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(GIT_BASH_DEFECTO));
    if !ruta.exists() {
        return Err(CoolifyError::Validation(format!(
            "No existe Git Bash en '{}' (define CM_GIT_BASH)",
            ruta.display()
        )));
    }
    Ok(ruta)
}

/* Ruta Windows → forma que entiende el bash de MSYS. */
fn ruta_bash(ruta: &Path) -> String {
    ruta.display().to_string().replace('\\', "/")
}

/* Error con la cola del log remoto para diagnosticar sin entrar por SSH. */
async fn cola_remota(ssh: &mut SshClient, etapa: &str, log: &str, exit_code: i32) -> CoolifyError {
    /* execute_long_running conserva el log si falla: aquí se lee y se borra. */
    let cola = ssh
        .execute(&format!(
            "tail -n 30 {0} 2>/dev/null; rm -f {0}",
            sh_quote(log)
        ))
        .await
        .map(|out| out.stdout)
        .unwrap_or_else(|_| "(no se pudo leer el log remoto)".into());
    CoolifyError::Validation(format!(
        "{etapa} en la VPS de build falló (exit {exit_code}). Cola:\n{}",
        cola.trim()
    ))
}

fn ultimas_lineas(texto: &str) -> String {
    let lineas: Vec<&str> = texto.lines().collect();
    let desde = lineas.len().saturating_sub(30);
    lineas[desde..].join("\n").trim().to_string()
}

/* Comillas simples seguras para `sh -c` (el comando remoto pasa por sh). */
fn sh_quote(valor: &str) -> String {
    format!("'{}'", valor.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /* Solo entran las líneas de la IP pedida, planas o con puerto; los hashes no casan. */
    #[test]
    fn known_hosts_filtra_por_ip_sin_hashes() {
        let kh = "66.94.100.241 ssh-ed25519 AAAA1\n\
                  |1|hash= ssh-ed25519 AAAA2\n\
                  [66.94.100.241]:22 ssh-rsa AAAA3\n\
                  89.117.16.170 ssh-ed25519 AAAA4\n\
                  # comentario 66.94.100.241\n";
        let lineas = lineas_known_hosts(kh, "66.94.100.241");
        assert_eq!(lineas.len(), 2);
        assert!(lineas
            .iter()
            .all(|l| l.contains("AAAA1") || l.contains("AAAA3")));
    }

    /* El script de la 89 verifica el sha antes de enviar, usa known_hosts temporal y limpia en trap. */
    #[test]
    fn script_89_verifica_sha_y_limpia_en_trap() {
        let imagen = ImagenRemota {
            remote_dir: "/tmp/cm-builder/inmobiliaria".into(),
            remote_tgz: "/tmp/cm-builder/inmobiliaria/imagen.tgz".into(),
            bytes: 1,
            sha256: "a".repeat(64),
        };
        let kh = vec!["66.94.100.241 ssh-ed25519 AAAA1".to_string()];
        let s = script_relevo_89(&imagen, "root@66.94.100.241", &kh, "/tmp/cm-relay-x.tgz");
        assert!(s.contains("sha256sum"));
        assert!(s.contains("exit 20"));
        assert!(s.contains("UserKnownHostsFile"));
        assert!(s.contains("StrictHostKeyChecking=yes"));
        assert!(s.contains("trap 'rm -f"));
        assert!(!s.contains("password"));
    }
}
