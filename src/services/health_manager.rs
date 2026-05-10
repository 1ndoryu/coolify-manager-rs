use crate::config::Settings;
use crate::domain::CommandOutput;
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

    /* [F11] Verificar que la respuesta HTML contiene indicadores del tema correcto.
     * Un sitio puede devolver HTTP 200 pero con tema incorrecto (twentytwentyfive, etc.)
     * si el contenedor fue recreado y el tema Glory se perdio. */
    let theme_content_ok = if http_ok && !body_text.is_empty() {
        match site.template {
            crate::domain::StackTemplate::Wordpress | crate::domain::StackTemplate::Kamples => {
                /* Buscar indicadores del tema Glory en el HTML */
                let has_glory_indicator = body_text.contains("glorytemplate")
                    || body_text.contains("glory-theme")
                    || body_text.contains(&site.theme_name)
                    || body_text.contains("/wp-content/themes/glorytemplate/");
                let has_default_theme =
                    body_text.contains("twentytwenty") || body_text.contains("starter theme");
                if !has_glory_indicator && has_default_theme {
                    details.push(format!(
                        "WARN: Tema incorrecto detectado. Se esperaba '{}' pero el HTML sugiere un tema por defecto",
                        site.theme_name
                    ));
                    false
                } else if !has_glory_indicator && body_text.len() < 500 {
                    details.push(
                        "WARN: Respuesta HTML sospechosamente corta, posible tema faltante"
                            .to_string(),
                    );
                    false
                } else {
                    true
                }
            }
            _ => true,
        }
    } else {
        true /* Si no hay body o HTTP fallo, no podemos verificar contenido */
    };

    let app_ok = match site.template {
        crate::domain::StackTemplate::Minecraft => {
            let result =
                docker::docker_exec(ssh, &app_container, "test -d /data && echo ok || echo fail")
                    .await?;
            result.stdout.trim() == "ok"
        }
        crate::domain::StackTemplate::Rust => {
            /* [105A-1] Rust apps: localhost dentro del contenedor puede responder aunque
             * Traefik no pueda alcanzar la IP Docker. Probamos host -> IP del contenedor
             * en la red del stack para detectar ese falso healthy antes de cerrar deploy. */
            let network_probe = run_rust_network_probe(
                ssh,
                stack_uuid,
                &app_container,
                &site.health_check.http_path,
            )
            .await?;
            if let Some(detail) = network_probe.detail {
                details.push(detail);
            }
            network_probe.ok
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
        app_ok: app_ok && theme_content_ok,
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

struct RustNetworkProbe {
    ok: bool,
    detail: Option<String>,
}

async fn run_rust_network_probe(
    ssh: &SshClient,
    stack_uuid: &str,
    app_container: &str,
    health_path: &str,
) -> std::result::Result<RustNetworkProbe, CoolifyError> {
    let command = rust_network_probe_command(stack_uuid, app_container, health_path);
    let result = ssh.execute(&command).await?;

    if result.success() && result.stdout.contains("RUST_NETWORK_OK") {
        return Ok(RustNetworkProbe {
            ok: true,
            detail: None,
        });
    }

    Ok(RustNetworkProbe {
        ok: false,
        detail: Some(format!(
            "Rust network probe fallo: {}",
            compact_probe_output(&result)
        )),
    })
}

fn rust_network_probe_command(stack_uuid: &str, app_container: &str, health_path: &str) -> String {
    let network = shell_single_quote(stack_uuid);
    let container = shell_single_quote(app_container);
    let path = shell_single_quote(&normalize_health_path(health_path));

    format!(
        "network={network}; container={container}; path={path}; \
         app_ip=$(docker inspect -f \"{{{{with index .NetworkSettings.Networks \\\"$network\\\"}}}}{{{{.IPAddress}}}}{{{{end}}}}\" \"$container\" 2>/dev/null); \
         if [ -z \"$app_ip\" ]; then app_ip=$(docker inspect -f \"{{{{range .NetworkSettings.Networks}}}}{{{{.IPAddress}}}} {{{{end}}}}\" \"$container\" 2>/dev/null | awk '{{print $1}}'); fi; \
         if [ -z \"$app_ip\" ]; then echo RUST_NETWORK_FAIL missing_ip; exit 1; fi; \
         curl -sf --max-time 5 \"http://$app_ip:3000$path\" >/dev/null && echo \"RUST_NETWORK_OK ip=$app_ip path=$path\" || (code=$?; echo \"RUST_NETWORK_FAIL ip=$app_ip path=$path exit=$code\"; exit 1)"
    )
}

fn normalize_health_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        "/".to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn compact_probe_output(result: &CommandOutput) -> String {
    let stdout = result.stdout.trim();
    let stderr = result.stderr.trim();
    let mut parts = vec![format!("exit={}", result.exit_code)];
    if !stdout.is_empty() {
        parts.push(format!("stdout={stdout}"));
    }
    if !stderr.is_empty() {
        parts.push(format!("stderr={stderr}"));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_network_probe_command_targets_container_network_ip() {
        let command = rust_network_probe_command("stack-uuid", "app-container", "/api/health");

        assert!(command.contains("NetworkSettings.Networks"));
        assert!(command.contains("stack-uuid"));
        assert!(command.contains("app-container"));
        assert!(command.contains("http://$app_ip:3000$path"));
        assert!(!command.contains("localhost:3000"));
    }

    #[test]
    fn normalize_health_path_adds_leading_slash() {
        assert_eq!(normalize_health_path("api/health"), "/api/health");
        assert_eq!(normalize_health_path("/api/health"), "/api/health");
        assert_eq!(normalize_health_path(""), "/");
    }

    #[test]
    fn shell_single_quote_escapes_quotes() {
        assert_eq!(shell_single_quote("stack'uuid"), "'stack'\\''uuid'");
    }
}
