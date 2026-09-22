/* Split 119A-5 de lightweight_runtime_manager.rs — control del ciclo de vida.
 * Codigo verbatim del original; solo cambia visibilidad de ayudantes compartidos. */

use super::plantillas::sh_quote;
use super::sitios::{action_report, require_site, resolve_compose_file};
use super::tipos::{LightweightSiteAction, LightweightSiteActionReport};
use crate::config::DeploymentTargetConfig;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

pub async fn control_lightweight_site(
    target: &DeploymentTargetConfig,
    site_name: &str,
    action: LightweightSiteAction,
    fqdn: Option<&str>,
    access_user: Option<&str>,
    access_password: Option<&str>,
    delete_volumes: bool,
) -> std::result::Result<LightweightSiteActionReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    match action {
        LightweightSiteAction::Delete => {
            delete_site(&ssh, site_name, delete_volumes).await?;
            Ok(LightweightSiteActionReport {
                target: target.name.clone(),
                target_ip: target.vps.ip.clone(),
                site: site_name.to_string(),
                action: action.as_str().to_string(),
                status: "deleted".to_string(),
                fqdn: None,
                notes: vec![if delete_volumes {
                    "Sitio eliminado junto con su directorio de proyecto.".to_string()
                } else {
                    "Sitio desmontado y proyecto preservado bajo /srv/backups/hosting/deleted/."
                        .to_string()
                }],
            })
        }
        LightweightSiteAction::Reconfigure => {
            reconfigure_site(&ssh, site_name, fqdn, access_user, access_password).await?;
            let site = require_site(&ssh, site_name).await?;
            let mut notes = Vec::new();
            if fqdn.is_some() {
                notes.push("Dominio/Caddy actualizado para el sitio lightweight.".to_string());
            }
            if access_password.is_some() {
                notes.push("Password SFTP actualizada en el host compartido.".to_string());
            }
            Ok(action_report(target, site, action, notes))
        }
        _ => {
            run_compose_action(&ssh, site_name, action).await?;
            let site = require_site(&ssh, site_name).await?;
            let mut notes = Vec::new();
            if action == LightweightSiteAction::Restart {
                notes.push(
                    "Si no habia contenedores previos, el compose se levantó en modo up -d."
                        .to_string(),
                );
            }
            Ok(action_report(target, site, action, notes))
        }
    }
}

async fn run_compose_action(
    ssh: &SshClient,
    site_name: &str,
    action: LightweightSiteAction,
) -> std::result::Result<(), CoolifyError> {
    let compose_file = resolve_compose_file(ssh, site_name).await?;
    let action_line = match action {
        LightweightSiteAction::Start => {
            "compose -p \"$site\" -f \"$compose_file\" up -d".to_string()
        }
        LightweightSiteAction::Stop => {
            "compose -p \"$site\" -f \"$compose_file\" stop".to_string()
        }
        LightweightSiteAction::Restart => "compose -p \"$site\" -f \"$compose_file\" restart >/dev/null 2>&1 || compose -p \"$site\" -f \"$compose_file\" up -d".to_string(),
        LightweightSiteAction::Reconfigure => unreachable!("reconfigure usa reconfigure_site"),
        LightweightSiteAction::Delete => unreachable!("delete usa delete_site"),
    };

    let script = vec![
        "set -euo pipefail".to_string(),
        "compose() {".to_string(),
        "  if docker compose version >/dev/null 2>&1; then".to_string(),
        "    docker compose \"$@\"".to_string(),
        "  elif command -v docker-compose >/dev/null 2>&1; then".to_string(),
        "    docker-compose \"$@\"".to_string(),
        "  else".to_string(),
        "    echo \"docker compose no esta disponible\" >&2".to_string(),
        "    exit 1".to_string(),
        "  fi".to_string(),
        "}".to_string(),
        format!("site={}", sh_quote(site_name)),
        format!("compose_file={}", sh_quote(&compose_file)),
        action_line,
    ]
    .join("\n");

    let output = ssh
        .execute(&format!("bash -lc {}", sh_quote(&script)))
        .await?;
    if !output.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo ejecutando '{}' sobre '{}': {}",
            action.as_str(),
            site_name,
            output.stderr
        )));
    }
    Ok(())
}

async fn delete_site(
    ssh: &SshClient,
    site_name: &str,
    delete_volumes: bool,
) -> std::result::Result<(), CoolifyError> {
    let compose_file = resolve_compose_file(ssh, site_name).await?;
    let delete_flag = if delete_volumes { "-v" } else { "" };
    let script = vec![
        "set -euo pipefail".to_string(),
        "compose() {".to_string(),
        "  if docker compose version >/dev/null 2>&1; then".to_string(),
        "    docker compose \"$@\"".to_string(),
        "  elif command -v docker-compose >/dev/null 2>&1; then".to_string(),
        "    docker-compose \"$@\"".to_string(),
        "  else".to_string(),
        "    echo \"docker compose no esta disponible\" >&2".to_string(),
        "    exit 1".to_string(),
        "  fi".to_string(),
        "}".to_string(),
        format!("site={}", sh_quote(site_name)),
        format!("compose_file={}", sh_quote(&compose_file)),
        "site_root=\"/srv/hosting/$site\"".to_string(),
        "deleted_root=\"/srv/backups/hosting/deleted\"".to_string(),
        "metadata_file=\"$site_root/site.env\"".to_string(),
        format!(
            "compose -p \"$site\" -f \"$compose_file\" down --remove-orphans {} >/dev/null 2>&1 || true",
            delete_flag
        ),
        "rm -f \"/etc/caddy/sites-enabled/$site.caddy\" \"/etc/caddy/sites-available/$site.caddy\"".to_string(),
        "if [ -f \"$metadata_file\" ]; then . \"$metadata_file\"; fi".to_string(),
        "if [ -n \"${LIGHT_SITE_USER:-}\" ]; then userdel \"$LIGHT_SITE_USER\" >/dev/null 2>&1 || true; fi".to_string(),
        "rm -f \"/etc/ssh/sshd_config.d/hosting-$site.conf\"".to_string(),
        "systemctl reload ssh >/dev/null 2>&1 || systemctl reload sshd >/dev/null 2>&1 || true".to_string(),
        "systemctl reload caddy >/dev/null 2>&1 || caddy reload --config /etc/caddy/Caddyfile >/dev/null 2>&1 || true".to_string(),
        if delete_volumes {
            "rm -rf \"$site_root\"".to_string()
        } else {
            [
                "mkdir -p \"$deleted_root\"",
                "if [ -d \"$site_root\" ]; then",
                "  mv \"$site_root\" \"$deleted_root/$site-$(date +%Y%m%d%H%M%S)\"",
                "fi",
            ]
            .join("\n")
        },
    ]
    .join("\n");

    let output = ssh
        .execute(&format!("bash -lc {}", sh_quote(&script)))
        .await?;
    if !output.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo eliminando '{}' del runtime lightweight: {}",
            site_name, output.stderr
        )));
    }
    Ok(())
}

async fn reconfigure_site(
    ssh: &SshClient,
    site_name: &str,
    fqdn: Option<&str>,
    access_user: Option<&str>,
    access_password: Option<&str>,
) -> std::result::Result<(), CoolifyError> {
    /* [245A-8] Reconfigure actualiza el contrato operativo minimo del sitio lightweight
     * sin recrearlo: dominio/Caddy y password SFTP. Cambiar access_user sigue bloqueado
     * porque implicaria rehacer ownership y jail del sitio en el host compartido. */
    let site_root = format!("/srv/hosting/{site_name}");
    let metadata_file = format!("{site_root}/site.env");
    let caddy_available = format!("/etc/caddy/sites-available/{site_name}.caddy");
    let caddy_enabled = format!("/etc/caddy/sites-enabled/{site_name}.caddy");
    let requested_fqdn = fqdn.map(str::trim).filter(|value| !value.is_empty());
    let requested_user = access_user.map(str::trim).filter(|value| !value.is_empty());
    let requested_password = access_password
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let script = vec![
        "set -euo pipefail".to_string(),
        format!("metadata_file={}", sh_quote(&metadata_file)),
        format!("caddy_available={}", sh_quote(&caddy_available)),
        format!("caddy_enabled={}", sh_quote(&caddy_enabled)),
        "if [ ! -f \"$metadata_file\" ]; then echo \"Metadatos del sitio no encontrados\" >&2; exit 14; fi".to_string(),
        ". \"$metadata_file\"".to_string(),
        match requested_fqdn {
            Some(value) => format!("next_fqdn={}", sh_quote(value)),
            None => "next_fqdn=\"$LIGHT_SITE_FQDN\"".to_string(),
        },
        match requested_user {
            Some(value) => format!("requested_user={}", sh_quote(value)),
            None => "requested_user=\"$LIGHT_SITE_USER\"".to_string(),
        },
        match requested_password {
            Some(value) => format!("requested_password={}", sh_quote(value)),
            None => "requested_password=''".to_string(),
        },
        "if [ \"$requested_user\" != \"$LIGHT_SITE_USER\" ]; then echo \"El runtime lightweight aun no soporta cambiar access_user\" >&2; exit 15; fi".to_string(),
        "if [ -n \"$requested_password\" ]; then printf '%s:%s\n' \"$LIGHT_SITE_USER\" \"$requested_password\" | chpasswd; fi".to_string(),
        "cat > \"$caddy_available\" <<EOF".to_string(),
        "$next_fqdn {".to_string(),
        "    encode zstd gzip".to_string(),
        "    reverse_proxy 127.0.0.1:$LIGHT_SITE_HTTP_PORT".to_string(),
        "    header {".to_string(),
        "        X-Content-Type-Options nosniff".to_string(),
        "        X-Frame-Options SAMEORIGIN".to_string(),
        "        Referrer-Policy strict-origin-when-cross-origin".to_string(),
        "    }".to_string(),
        "}".to_string(),
        "EOF".to_string(),
        "cat > \"$metadata_file\" <<EOF".to_string(),
        "LIGHT_SITE_USER=$LIGHT_SITE_USER".to_string(),
        "LIGHT_SITE_HTTP_PORT=$LIGHT_SITE_HTTP_PORT".to_string(),
        "LIGHT_SITE_FQDN=$next_fqdn".to_string(),
        "EOF".to_string(),
        "ln -sfn \"$caddy_available\" \"$caddy_enabled\"".to_string(),
        "caddy validate --config /etc/caddy/Caddyfile >/dev/null".to_string(),
        "systemctl reload caddy >/dev/null 2>&1 || caddy reload --config /etc/caddy/Caddyfile >/dev/null 2>&1".to_string(),
    ]
    .join("\n");

    let output = ssh
        .execute(&format!("bash -lc {}", sh_quote(&script)))
        .await?;
    if !output.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo reconfigurando '{}' del runtime lightweight: {}",
            site_name, output.stderr
        )));
    }
    Ok(())
}
