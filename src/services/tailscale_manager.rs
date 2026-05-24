use crate::config::{DeploymentTargetConfig, Settings, VpsConfig};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;

const TAILSCALE_INSTALL_CMD: &str = r#"set -e
if ! command -v curl >/dev/null 2>&1; then
  if command -v apt-get >/dev/null 2>&1; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq >/dev/null 2>&1 || true
    apt-get install -y -qq curl ca-certificates >/dev/null 2>&1
  elif command -v dnf >/dev/null 2>&1; then
    dnf install -y curl ca-certificates >/dev/null 2>&1
  elif command -v yum >/dev/null 2>&1; then
    yum install -y curl ca-certificates >/dev/null 2>&1
  else
    echo NO_SUPPORTED_PACKAGE_MANAGER
    exit 1
  fi
fi
if ! command -v tailscale >/dev/null 2>&1; then
  curl -fsSL https://tailscale.com/install.sh | sh
fi
systemctl enable --now tailscaled >/dev/null 2>&1 || true
command -v tailscale >/dev/null 2>&1
"#;

#[derive(Debug, Clone)]
pub struct TailscaleBootstrapRequest {
    pub auth_key: Option<String>,
    pub hostname: Option<String>,
    pub advertise_tags: Option<String>,
    pub accept_dns: bool,
    pub probe_url: Option<String>,
    pub probe_method: String,
    pub probe_body: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TailscaleBootstrapReport {
    pub target: String,
    pub os_name: String,
    pub installed: bool,
    pub installed_now: bool,
    pub authenticated: bool,
    pub tailscale_ip: Option<String>,
    pub login_url: Option<String>,
    pub http_probe: Option<HttpProbeReport>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HttpProbeReport {
    pub url: String,
    pub method: String,
    pub ok: bool,
    pub status_code: Option<u16>,
    pub body: String,
    pub detail: String,
}

pub async fn bootstrap_default_vps(
    settings: &Settings,
    request: &TailscaleBootstrapRequest,
) -> std::result::Result<TailscaleBootstrapReport, CoolifyError> {
    bootstrap_vps_config("default", &settings.vps, request).await
}

pub async fn bootstrap_target(
    target: &DeploymentTargetConfig,
    request: &TailscaleBootstrapRequest,
) -> std::result::Result<TailscaleBootstrapReport, CoolifyError> {
    bootstrap_vps_config(&target.name, &target.vps, request).await
}

pub async fn bootstrap_vps_config(
    target_name: &str,
    vps: &VpsConfig,
    request: &TailscaleBootstrapRequest,
) -> std::result::Result<TailscaleBootstrapReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(vps);
    ssh.connect().await?;

    let os_name = ssh
        .execute("bash -lc 'source /etc/os-release 2>/dev/null && echo ${PRETTY_NAME:-unknown}'")
        .await?
        .stdout
        .trim()
        .to_string();

    let installed_before = tailscale_installed(&ssh).await?;
    if !installed_before {
        install_tailscale(&ssh, target_name).await?;
    }
    let installed_after = tailscale_installed(&ssh).await?;
    if !installed_after {
        return Err(CoolifyError::Validation(format!(
            "No se pudo instalar Tailscale en '{target_name}'"
        )));
    }

    let mut notes = vec![format!(
        "SO detectado: {}",
        if os_name.is_empty() {
            "unknown"
        } else {
            &os_name
        }
    )];
    if installed_before {
        notes.push("Tailscale ya estaba instalado en el host.".to_string());
    } else {
        notes.push("Tailscale instalado y servicio tailscaled arrancado.".to_string());
    }

    let mut tailscale_ip = fetch_tailscale_ip(&ssh).await?;
    let mut authenticated = tailscale_ip.is_some();
    let mut login_url = None;

    if !authenticated {
        let up_result = run_tailscale_up(&ssh, request).await?;
        let combined_output = combine_output(&up_result.stdout, &up_result.stderr);
        login_url = extract_login_url(&combined_output);

        tailscale_ip = fetch_tailscale_ip(&ssh).await?;
        authenticated = tailscale_ip.is_some();

        if authenticated {
            notes.push("El host ya quedo autenticado en el tailnet.".to_string());
        } else if let Some(url) = &login_url {
            notes.push(format!(
                "Autenticacion pendiente: abre la URL de login y vuelve a ejecutar el comando. {}",
                url
            ));
        } else if request.auth_key.is_some() {
            return Err(CoolifyError::Validation(format!(
                "Tailscale no obtuvo IP despues de 'tailscale up' en '{target_name}'. Salida: {}",
                combined_output.trim()
            )));
        } else {
            notes.push(
                "Tailscale sigue sin autenticarse y no devolvio una URL de login reutilizable."
                    .to_string(),
            );
        }
    } else {
        notes.push("El host ya estaba autenticado en el tailnet.".to_string());
    }

    let http_probe = if authenticated {
        if let Some(url) = request.probe_url.as_deref() {
            let probe = http_probe(
                &ssh,
                url,
                &request.probe_method,
                request.probe_body.as_deref(),
            )
            .await?;
            if probe.ok {
                notes.push(
                    "El host puede alcanzar el endpoint remoto solicitado por Tailscale."
                        .to_string(),
                );
            } else {
                notes.push("El host ya tiene Tailscale, pero el probe HTTP al endpoint remoto sigue fallando.".to_string());
            }
            Some(probe)
        } else {
            None
        }
    } else {
        if request.probe_url.is_some() {
            notes.push(
                "Probe HTTP omitido hasta completar la autenticacion de Tailscale.".to_string(),
            );
        }
        None
    };

    notes.push(
        "Si el host resuelve por Tailscale, los contenedores Docker del VPS deberian heredar reachability via NAT del bridge.".to_string(),
    );

    Ok(TailscaleBootstrapReport {
        target: target_name.to_string(),
        os_name,
        installed: installed_after,
        installed_now: !installed_before && installed_after,
        authenticated,
        tailscale_ip,
        login_url,
        http_probe,
        notes,
    })
}

async fn tailscale_installed(ssh: &SshClient) -> std::result::Result<bool, CoolifyError> {
    let result = ssh
        .execute("bash -lc 'command -v tailscale >/dev/null 2>&1 && echo INSTALLED || true'")
        .await?;
    Ok(result.stdout.contains("INSTALLED"))
}

async fn install_tailscale(
    ssh: &SshClient,
    target_name: &str,
) -> std::result::Result<(), CoolifyError> {
    let command = format!("bash -lc {}", sh_quote(TAILSCALE_INSTALL_CMD));
    let result = ssh.execute(&command).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo instalando Tailscale en '{target_name}': {}{}",
            result.stdout, result.stderr
        )));
    }
    Ok(())
}

async fn fetch_tailscale_ip(ssh: &SshClient) -> std::result::Result<Option<String>, CoolifyError> {
    let result = ssh
        .execute("bash -lc 'tailscale ip -4 2>/dev/null | head -1 || true'")
        .await?;
    let ip = result.stdout.trim();
    if ip.is_empty() {
        Ok(None)
    } else {
        Ok(Some(ip.to_string()))
    }
}

async fn run_tailscale_up(
    ssh: &SshClient,
    request: &TailscaleBootstrapRequest,
) -> std::result::Result<crate::domain::CommandOutput, CoolifyError> {
    let tailscale_cmd = build_tailscale_up_command(request);
    let wrapped = format!("bash -lc {}", sh_quote(&tailscale_cmd));
    ssh.execute(&wrapped).await
}

fn build_tailscale_up_command(request: &TailscaleBootstrapRequest) -> String {
    let mut tokens = vec![
        "tailscale".to_string(),
        "up".to_string(),
        format!(
            "--accept-dns={}",
            if request.accept_dns { "true" } else { "false" }
        ),
    ];
    if let Some(hostname) = request.hostname.as_deref() {
        tokens.push(format!("--hostname={hostname}"));
    }
    if let Some(tags) = request.advertise_tags.as_deref() {
        tokens.push(format!("--advertise-tags={tags}"));
    }
    if let Some(auth_key) = request.auth_key.as_deref() {
        tokens.push(format!("--auth-key={auth_key}"));
    }

    let raw = tokens
        .iter()
        .map(|token| sh_quote(token))
        .collect::<Vec<_>>()
        .join(" ");

    if request.auth_key.is_some() {
        raw
    } else {
        format!("timeout 20 {raw} 2>&1 || true")
    }
}

async fn http_probe(
    ssh: &SshClient,
    url: &str,
    method: &str,
    body: Option<&str>,
) -> std::result::Result<HttpProbeReport, CoolifyError> {
    let normalized_method = method.trim().to_uppercase();
    let mut tokens = vec![
        "curl".to_string(),
        "-sS".to_string(),
        "--max-time".to_string(),
        "10".to_string(),
        "-X".to_string(),
        normalized_method.clone(),
    ];

    if let Some(payload) = body {
        tokens.push("-H".to_string());
        tokens.push("Content-Type: application/json".to_string());
        tokens.push("--data".to_string());
        tokens.push(payload.to_string());
    }

    tokens.push("-w".to_string());
    tokens.push("\\nCURL_HTTP_CODE:%{http_code}".to_string());
    tokens.push(url.to_string());

    let inner = tokens
        .iter()
        .map(|token| sh_quote(token))
        .collect::<Vec<_>>()
        .join(" ");
    let command = format!("bash -lc {}", sh_quote(&inner));
    let result = ssh.execute(&command).await?;

    let mut body_output = result.stdout.clone();
    let mut status_code = None;
    if let Some((body_part, code_part)) = result.stdout.rsplit_once("CURL_HTTP_CODE:") {
        body_output = body_part.trim_end().to_string();
        status_code = code_part.trim().parse::<u16>().ok();
    }

    Ok(HttpProbeReport {
        url: url.to_string(),
        method: normalized_method,
        ok: result.success() && status_code.is_some_and(|code| code < 400),
        status_code,
        body: body_output,
        detail: combine_output(&result.stderr, ""),
    })
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn combine_output(first: &str, second: &str) -> String {
    match (first.trim(), second.trim()) {
        ("", "") => String::new(),
        (left, "") => left.to_string(),
        ("", right) => right.to_string(),
        (left, right) => format!("{left}\n{right}"),
    }
}

fn extract_login_url(output: &str) -> Option<String> {
    login_url_regex()
        .captures(output)
        .and_then(|caps| caps.get(0))
        .map(|value| value.as_str().to_string())
}

fn login_url_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"https://login\.tailscale\.com/[^\s'"]+"#)
            .expect("regex tailscale login valida")
    })
}

#[cfg(test)]
mod tests {
    use super::{
        build_tailscale_up_command, extract_login_url, sh_quote, TailscaleBootstrapRequest,
    };

    fn request() -> TailscaleBootstrapRequest {
        TailscaleBootstrapRequest {
            auth_key: None,
            hostname: Some("coolify-vps1".to_string()),
            advertise_tags: Some("tag:vps".to_string()),
            accept_dns: false,
            probe_url: None,
            probe_method: "POST".to_string(),
            probe_body: None,
        }
    }

    #[test]
    fn detecta_login_url_tailscale() {
        let output = "To authenticate, visit:\nhttps://login.tailscale.com/a/abcdef\n";
        assert_eq!(
            extract_login_url(output).as_deref(),
            Some("https://login.tailscale.com/a/abcdef")
        );
    }

    #[test]
    fn tailscale_up_interactivo_usa_timeout() {
        let command = build_tailscale_up_command(&request());
        assert!(command.starts_with("timeout 20 'tailscale' 'up'"));
        assert!(command.contains("--hostname=coolify-vps1"));
        assert!(command.contains("--advertise-tags=tag:vps"));
        assert!(command.contains("--accept-dns=false"));
    }

    #[test]
    fn shell_quote_escapa_comillas_simples() {
        assert_eq!(sh_quote("a'b"), "'a'\\''b'");
    }
}
