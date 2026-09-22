use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::tailscale_manager::{self, TailscaleBootstrapRequest};

use std::path::Path;

/* Params del comando tailscale (119A-6). Todo Copy: `= *p` sin mover. */
#[derive(Clone, Copy)]
pub struct ParamsTailscale<'a> {
    pub config_path: &'a Path,
    pub target_name: Option<&'a str>,
    pub auth_key: Option<&'a str>,
    pub auth_key_env: Option<&'a str>,
    pub hostname: Option<&'a str>,
    pub advertise_tags: Option<&'a str>,
    pub accept_dns: bool,
    pub probe_url: Option<&'a str>,
    pub probe_method: &'a str,
    pub probe_body: Option<&'a str>,
}

pub async fn execute(p: &ParamsTailscale<'_>) -> std::result::Result<(), CoolifyError> {
    let ParamsTailscale {
        config_path,
        target_name,
        auth_key,
        auth_key_env,
        hostname,
        advertise_tags,
        accept_dns,
        probe_url,
        probe_method,
        probe_body,
    } = *p;
    let settings = Settings::load(config_path)?;
    let target = match target_name {
        Some(name) => settings.get_target(name)?.clone(),
        None => settings.default_target(),
    };
    let resolved_auth_key = resolve_auth_key(auth_key, auth_key_env);

    let request = TailscaleBootstrapRequest {
        auth_key: resolved_auth_key,
        hostname: hostname.map(ToOwned::to_owned),
        advertise_tags: advertise_tags.map(ToOwned::to_owned),
        accept_dns,
        probe_url: probe_url.map(ToOwned::to_owned),
        probe_method: probe_method.to_string(),
        probe_body: probe_body.map(ToOwned::to_owned),
    };

    let report = tailscale_manager::bootstrap_target(&target, &request).await?;

    println!("Target: {}", report.target);
    println!("SO: {}", report.os_name);
    println!(
        "Tailscale instalado: {}",
        if report.installed_now {
            "instalado ahora"
        } else if report.installed {
            "ya estaba"
        } else {
            "no"
        }
    );
    println!(
        "Autenticado: {}",
        if report.authenticated {
            "si"
        } else {
            "pendiente"
        }
    );
    println!(
        "IP Tailscale: {}",
        report.tailscale_ip.as_deref().unwrap_or("pendiente")
    );
    if let Some(login_url) = &report.login_url {
        println!("Login URL: {login_url}");
    }
    if let Some(probe) = &report.http_probe {
        println!(
            "Probe HTTP: {} {} -> {}",
            probe.method,
            probe.url,
            if probe.ok { "OK" } else { "FALLO" }
        );
        if let Some(status) = probe.status_code {
            println!("HTTP status: {status}");
        }
        if !probe.body.trim().is_empty() {
            println!("HTTP body: {}", probe.body.trim());
        }
        if !probe.detail.trim().is_empty() {
            println!("HTTP detalle: {}", probe.detail.trim());
        }
    }
    for note in report.notes {
        println!("- {note}");
    }

    Ok(())
}

fn resolve_auth_key(auth_key: Option<&str>, auth_key_env: Option<&str>) -> Option<String> {
    auth_key
        .map(ToOwned::to_owned)
        .or_else(|| auth_key_env.and_then(read_env_var))
        .or_else(|| read_env_var("TAILSCALE_AUTH_KEY"))
        .or_else(|| read_env_var("GLORY_TAILSCALE_AUTH_KEY"))
}

fn read_env_var(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
