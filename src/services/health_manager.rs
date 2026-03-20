use crate::config::Settings;
use crate::domain::SiteConfig;
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;
use crate::services::site_capabilities;

use reqwest::redirect::Policy;
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub site_name: String,
    pub url: String,
    pub http_ok: bool,
    pub app_ok: bool,
    pub fatal_log_detected: bool,
    pub status_code: Option<u16>,
    pub details: Vec<String>,
}

impl HealthReport {
    pub fn healthy(&self) -> bool {
        self.http_ok && self.app_ok && !self.fatal_log_detected
    }
}

pub async fn run_site_health_check(
    _settings: &Settings,
    site: &SiteConfig,
    ssh: &SshClient,
) -> std::result::Result<HealthReport, CoolifyError> {
    let caps = site_capabilities::resolve(site);
    let stack_uuid = site.stack_uuid.as_deref().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{}' sin stackUuid", site.nombre))
    })?;
    let app_container = caps.resolve_app_container(ssh, stack_uuid).await?;
    let url = caps.health_url(site);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(site.health_check.timeout_seconds))
        .redirect(Policy::limited(5))
        .build()
        .map_err(|e| CoolifyError::Validation(format!("No se pudo crear cliente HTTP: {e}")))?;

    let mut details = Vec::new();
    let response = client.get(&url).send().await;
    let (http_ok, status_code, body_text) = match response {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            let ok = (200..400).contains(&status);
            if !ok {
                details.push(format!("HTTP devolvio estado {status}"));
            }
            (ok, Some(status), body)
        }
        Err(err) => {
            details.push(format!("Fallo HTTP: {err}"));
            (false, None, String::new())
        }
    };

    for pattern in &site.health_check.fatal_patterns {
        if body_text.contains(pattern) {
            details.push(format!("Respuesta HTTP contiene patron fatal: {pattern}"));
        }
    }

    let app_ok = match site.template {
        crate::domain::StackTemplate::Minecraft => {
            let result =
                docker::docker_exec(ssh, &app_container, "test -d /data && echo ok || echo fail")
                    .await?;
            result.stdout.trim() == "ok"
        }
        _ => {
            let result = docker::docker_exec(
                ssh,
                &app_container,
                "php -r \"require '/var/www/html/wp-load.php'; echo 'ok';\" 2>/dev/null || true",
            )
            .await?;
            result.stdout.trim() == "ok"
        }
    };

    if !app_ok {
        details.push("Chequeo interno de aplicacion fallo".to_string());
    }

    let log_probe = docker::docker_exec(
        ssh,
        &app_container,
        "tail -n 200 /var/log/apache2/error.log 2>/dev/null || tail -n 200 /var/www/html/wp-content/debug.log 2>/dev/null || true",
    )
    .await?;
    let fatal_log_detected =
        site.health_check.fatal_patterns.iter().any(|pattern| {
            log_probe.stdout.contains(pattern) || log_probe.stderr.contains(pattern)
        });

    if fatal_log_detected {
        details.push("Se detectaron patrones fatales en logs recientes".to_string());
    }

    let report = HealthReport {
        site_name: site.nombre.clone(),
        url,
        http_ok,
        app_ok,
        fatal_log_detected,
        status_code,
        details,
    };

    Ok(report)
}

pub async fn assert_site_healthy(
    settings: &Settings,
    site: &SiteConfig,
    ssh: &SshClient,
) -> std::result::Result<HealthReport, CoolifyError> {
    let report = run_site_health_check(settings, site, ssh).await?;
    if !report.healthy() {
        return Err(CoolifyError::Validation(format!(
            "Health check fallo para '{}': {}",
            site.nombre,
            report.details.join(" | ")
        )));
    }
    Ok(report)
}
