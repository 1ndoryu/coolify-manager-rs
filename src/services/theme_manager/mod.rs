/*
 * ThemeManager — instalacion y actualizacion del tema Glory.
 * Equivale a WordPress/ThemeManager.psm1.
 */

use crate::config::GloryConfig;
use crate::domain::{PhpConfig, SmtpConfig};
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;

async fn obtener_hash_archivo_remoto(
    ssh: &SshClient,
    container_id: &str,
    ruta: &str,
) -> Option<String> {
    let comando = format!("if [ -f {ruta} ]; then sha256sum {ruta} | awk '{{print $1}}'; fi");

    match docker::docker_exec(ssh, container_id, &comando).await {
        Ok(resultado) if resultado.success() => {
            let hash = resultado.stdout.trim();
            if hash.is_empty() {
                None
            } else {
                Some(hash.to_string())
            }
        }
        _ => None,
    }
}

/// Instala el tema Glory completo dentro del contenedor WordPress.

/* [119A-3] Contexto de `update_glory_theme`: agrupa los 12 parámetros + rutas
 * derivadas para que cada fase reciba solo `&CtxActualizacionTema`.
 * (Antes era una sola función de ~350 líneas; ahora es un orquestador fino.) */
struct CtxActualizacionTema<'a> {
    ssh: &'a SshClient,
    container_id: &'a str,
    stack_uuid: &'a str,
    glory_config: &'a GloryConfig,
    glory_branch: &'a str,
    library_branch: &'a str,
    theme_name: &'a str,
    skip_react: bool,
    force: bool,
    php_config: Option<&'a PhpConfig>,
    smtp_config: Option<&'a SmtpConfig>,
    disable_wp_cron: bool,
    theme_dir: String,
    glory_dir: String,
    composer_lock: String,
    npm_lock: String,
}

impl<'a> CtxActualizacionTema<'a> {
    #[allow(clippy::too_many_arguments)]
    fn nuevo(
        ssh: &'a SshClient,
        container_id: &'a str,
        stack_uuid: &'a str,
        glory_config: &'a GloryConfig,
        glory_branch: &'a str,
        library_branch: &'a str,
        theme_name: &'a str,
        skip_react: bool,
        force: bool,
        php_config: Option<&'a PhpConfig>,
        smtp_config: Option<&'a SmtpConfig>,
        disable_wp_cron: bool,
    ) -> Self {
        let theme_dir = format!("/var/www/html/wp-content/themes/{theme_name}");
        let glory_dir = format!("{theme_dir}/Glory");
        Self {
            ssh,
            container_id,
            stack_uuid,
            glory_config,
            glory_branch,
            library_branch,
            theme_name,
            skip_react,
            force,
            php_config,
            smtp_config,
            disable_wp_cron,
            composer_lock: format!("{theme_dir}/composer.lock"),
            npm_lock: format!("{theme_dir}/package-lock.json"),
            theme_dir,
            glory_dir,
        }
    }
}

pub use update::update_glory_theme;
/// Codifica un string en base64 (sin dependencia externa — usa solo std).
fn base64_encode(input: &str) -> String {
    use std::fmt::Write;
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        let n = (b0 << 16) | (b1 << 8) | b2;
        let _ = write!(
            out,
            "{}{}{}{}",
            TABLE[(n >> 18) & 0x3F] as char,
            TABLE[(n >> 12) & 0x3F] as char,
            if chunk.len() > 1 {
                TABLE[(n >> 6) & 0x3F] as char
            } else {
                '='
            },
            if chunk.len() > 2 {
                TABLE[n & 0x3F] as char
            } else {
                '='
            },
        );
    }
    out
}


mod fases;
mod install;
mod update;

pub use install::install_glory_theme;
