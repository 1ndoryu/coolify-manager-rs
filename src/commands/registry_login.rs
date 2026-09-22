/* [119A-4 F2] registry-login — autentica el Docker de la VPS contra un
 * registry privado (ghcr.io) para que `deploy-service` pueda hacer pull de
 * imágenes precompiladas sin build en la VPS.
 *
 * Seguridad del secreto (PAT):
 * - El token NUNCA viaja en argv ni se imprime en logs: se lee SOLO de la
 *   variable de entorno CM_REGISTRY_TOKEN.
 * - Se sube a la VPS como fichero temporal 0600, se consume por stdin de
 *   `docker login --password-stdin` y se borra en el mismo comando remoto
 *   (incluso si el login falla: `rc=$?; rm -f; exit $rc`).
 * - `host` y `user` se validan como tokens shell-safe (sin espacios,
 *   comillas ni metacaracteres) antes de interpolarlos en el comando remoto.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;

use clap::Args;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::info;

/// Nombre de la variable de entorno que porta el token (PAT `packages:read`).
pub const REGISTRY_TOKEN_ENV: &str = "CM_REGISTRY_TOKEN";

#[derive(Args)]
pub struct RegistryLoginArgs {
    /// Host del registry (p. ej. `ghcr.io`)
    #[arg(long, default_value = "ghcr.io")]
    pub host: String,

    /// Usuario del registry (p. ej. tu usuario de GitHub)
    pub user: String,

    /// Target de settings.json (por defecto el target primario)
    #[arg(long)]
    pub target: Option<String>,

    /// Muestra lo que haría sin ejecutar nada remoto
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,
}

/// Valida que un valor sea seguro para interpolar en un comando shell remoto:
/// solo ASCII alfanumérico más `. : - _`. Rechaza espacios, comillas,
/// expansiones (`$`, backticks) y separadores (`;`, `&`, `|`, ...).
pub fn validar_token_shell(valor: &str) -> Result<(), CoolifyError> {
    let vacio_o_inseguro = valor.is_empty()
        || !valor
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '_'));
    if vacio_o_inseguro {
        return Err(CoolifyError::Validation(format!(
            "valor de registry no permitido (solo [a-zA-Z0-9.:_-]): '{valor}'"
        )));
    }
    Ok(())
}

pub async fn run(settings: &Settings, args: &RegistryLoginArgs) -> Result<(), CoolifyError> {
    validar_token_shell(&args.host)?;
    validar_token_shell(&args.user)?;

    let (target_nombre, target_vps) = match args.target.as_deref() {
        Some(name) => {
            let target = settings.get_target(name)?;
            (target.name.clone(), &target.vps)
        }
        None => ("default".to_string(), &settings.vps),
    };
    info!(
        "Autenticando Docker del target '{}' ({}) contra registry '{}' como '{}'",
        target_nombre, target_vps.ip, args.host, args.user
    );

    if args.dry_run {
        println!(
            "[dry-run] docker login {} -u {} --password-stdin (token desde ${}) en {}",
            args.host, args.user, REGISTRY_TOKEN_ENV, target_vps.ip
        );
        return Ok(());
    }

    /* El token solo existe en memoria local y en el fichero temporal remoto.
     * Nunca se interpola en el comando ni se registra en logs. */
    let token = std::env::var(REGISTRY_TOKEN_ENV).map_err(|_| {
        CoolifyError::Validation(format!(
            "falta el token: exporta {REGISTRY_TOKEN_ENV} con un PAT con permiso packages:read"
        ))
    })?;
    if token.trim().is_empty() {
        return Err(CoolifyError::Validation(format!(
            "{REGISTRY_TOKEN_ENV} está vacío"
        )));
    }

    let mut ssh = SshClient::from_vps(target_vps);
    ssh.connect().await?;

    /* Fichero temporal local con el token (se borra justo tras subirlo). */
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    /* [119A-5] canonicalize: nombre temporal generado (nanos u32), sin input externo. */
    let tmp_name = format!("cm-reglogin-{}.token", nanos);
    validation::validar_segmento_ruta(&tmp_name, "temporal")?;
    let local_tmp = validation::join_segmento_seguro(&std::env::temp_dir(), &tmp_name, "temporal")?;
    std::fs::write(&local_tmp, token.as_bytes())?;
    // [119A-4] Limpieza local inmediata tras la subida (el remoto se borra
    // en el propio comando remoto, falle o no el login).
    let subida = ssh
        .upload_file_streamed(&local_tmp, "/tmp/cm-registry-token")
        .await;
    let _ = std::fs::remove_file(&local_tmp);
    subida?;

    /* 0600 + login por stdin + borrado garantizado del temporal remoto. */
    let cmd = format!(
        "chmod 600 /tmp/cm-registry-token && docker login {} -u {} --password-stdin < /tmp/cm-registry-token; rc=$?; rm -f /tmp/cm-registry-token; exit $rc",
        args.host, args.user
    );
    let out = ssh.execute(&cmd).await?;
    if !out.success() {
        /* Resumen del stderr (el token nunca aparece: viajo por stdin). */
        let resumen: Vec<&str> = out
            .stderr
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .take(8)
            .collect();
        return Err(CoolifyError::Validation(format!(
            "docker login contra {} falló (exit {}): {}",
            args.host,
            out.exit_code,
            resumen.join(" | ")
        )));
    }
    println!(
        "Registry '{}' autenticado como '{}' en '{}'.",
        args.host, args.user, target_nombre
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validar_token_shell;

    #[test]
    fn test_validar_token_shell_acepta_host_y_usuario() {
        assert!(validar_token_shell("ghcr.io").is_ok());
        assert!(validar_token_shell("1ndoryu").is_ok());
        assert!(validar_token_shell("registry.local:5000").is_ok());
    }

    #[test]
    fn test_validar_token_shell_rechaza_inyeccion() {
        assert!(validar_token_shell("").is_err());
        assert!(validar_token_shell("a;b").is_err());
        assert!(validar_token_shell("$(id)").is_err());
        assert!(validar_token_shell("`id`").is_err());
        assert!(validar_token_shell("a b").is_err());
        assert!(validar_token_shell("--password-stdin < /etc/passwd").is_err());
        assert!(validar_token_shell("user'quote").is_err());
    }
}
