/*
 * Comando: view-logs
 * Obtiene logs del contenedor o debug.log de WordPress.
 *
 * Soporta dos modos:
 * - SSH (por defecto): conecta al VPS via SSH y ejecuta `docker logs`.
 * - Docker API (--docker-socket): conecta al Docker daemon directamente.
 *   Útil cuando SSH está bloqueado (SSH Guard) o para integraciones sin clave.
 *
 * Ejemplo: cm logs --name studio --target mariadb --docker-socket tcp://66.94.100.241:2375
 */

use crate::config::Settings;
use crate::domain::StackTemplate;
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::docker_api::DockerApiClient;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    lines: u32,
    target: &str,
    wp_debug: bool,
    filter: Option<&str>,
    docker_socket: Option<&str>,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    let stack_uuid = site.stack_uuid.as_deref().unwrap();
    let effective_target = resolve_log_target(&site.template, target);

    if wp_debug && effective_target != "wordpress" {
        return Err(CoolifyError::Validation(
            "--wp-debug solo aplica a stacks WordPress".to_string(),
        ));
    }

    /* Modo Docker API: sin SSH, conecta directo al daemon */
    if let Some(socket) = docker_socket {
        return execute_via_docker_api(socket, stack_uuid, effective_target, lines, filter).await;
    }

    /* Modo SSH (comportamiento original) */
    let target_config = settings.resolve_site_target(site)?;
    let mut ssh = SshClient::from_vps(&target_config.vps);
    ssh.connect().await?;

    /* [114A-6] Soporte para target 'app' y 'websocket' en logs */
    let container_id = match effective_target {
        "site" => docker::find_site_container(&ssh, stack_uuid).await?,
        "mariadb" => docker::find_mariadb_container(&ssh, stack_uuid).await?,
        "postgres" => docker::find_postgres_container(&ssh, stack_uuid).await?,
        "app" => docker::find_app_container(&ssh, stack_uuid).await?,
        "websocket" => docker::find_websocket_container(&ssh, stack_uuid).await?,
        _ => docker::find_wordpress_container(&ssh, stack_uuid).await?,
    };

    let output = if wp_debug {
        /* Leer debug.log de WordPress */
        let mut cmd =
            format!("cat /var/www/html/wp-content/debug.log 2>/dev/null | tail -n {lines}");
        if let Some(pattern) = filter {
            cmd = format!(
                "cat /var/www/html/wp-content/debug.log 2>/dev/null | grep -i '{}' | tail -n {lines}",
                pattern.replace('\'', "'\\''")
            );
        }
        docker::docker_exec(&ssh, &container_id, &cmd).await?
    } else {
        /* Logs del contenedor Docker */
        let cmd = format!("docker logs --tail {lines} {container_id} 2>&1");
        ssh.execute(&cmd).await?
    };

    if output.stdout.is_empty() && output.stderr.is_empty() {
        println!("(sin logs disponibles)");
    } else {
        if !output.stdout.is_empty() {
            print!("{}", output.stdout);
        }
        if !output.stderr.is_empty() {
            eprint!("{}", output.stderr);
        }
    }

    Ok(())
}

/// Obtiene logs usando el Docker Engine API directamente (sin SSH).
/// Busca contenedores por fragmento de nombre que contenga el stack_uuid.
async fn execute_via_docker_api(
    socket: &str,
    stack_uuid: &str,
    target: &str,
    lines: u32,
    filter: Option<&str>,
) -> std::result::Result<(), CoolifyError> {
    let client = DockerApiClient::connect(Some(socket))?;

    /* Verificar conexión */
    client.ping().await?;

    /* Resolver nombre del contenedor objetivo.
     * Los contenedores de Coolify contienen el UUID del stack en su nombre.
     * Para cada target buscamos un contenedor que combine stack_uuid + target hint. */
    let name_hint = match target {
        "site" => stack_uuid.to_string(),
        "mariadb" => format!("{stack_uuid}-mariadb"),
        "postgres" => format!("{stack_uuid}-postgres"),
        "app" => format!("{stack_uuid}-app"),
        "websocket" => format!("{stack_uuid}-websocket"),
        _ => format!("{stack_uuid}-wordpress"),
    };

    /* Intentar buscar por hint específico, fallback a solo stack_uuid */
    let container_name = match client.resolve_container_name(&name_hint).await {
        Ok(name) => name,
        Err(_) => {
            /* Fallback: buscar cualquier contenedor del stack */
            let containers = client.find_containers(stack_uuid).await?;
            if containers.is_empty() {
                return Err(CoolifyError::DockerApi(format!(
                    "no se encontro contenedor para stack {stack_uuid} (target: {target})"
                )));
            }
            let best = containers
                .iter()
                .find(|c| {
                    c.names
                        .iter()
                        .any(|n| n.contains(target) || n.contains("wordpress") || n.contains("app"))
                })
                .unwrap_or(&containers[0]);

            best.names
                .first()
                .cloned()
                .unwrap_or_else(|| best.id.clone())
        }
    };

    eprintln!("(docker-api) contenedor: {container_name}");

    let log_output = client
        .container_logs(&container_name, lines, 0, true, true)
        .await?;

    /* Filtrar si se especificó patrón */
    let stdout = if let Some(pattern) = filter {
        log_output
            .stdout
            .lines()
            .filter(|line| line.to_lowercase().contains(&pattern.to_lowercase()))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        log_output.stdout
    };

    let stderr = if let Some(pattern) = filter {
        log_output
            .stderr
            .lines()
            .filter(|line| line.to_lowercase().contains(&pattern.to_lowercase()))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        log_output.stderr
    };

    if stdout.is_empty() && stderr.is_empty() {
        println!("(sin logs disponibles)");
    } else {
        if !stdout.is_empty() {
            print!("{stdout}");
            if !stdout.ends_with('\n') {
                println!();
            }
        }
        if !stderr.is_empty() {
            eprint!("{stderr}");
            if !stderr.ends_with('\n') {
                eprintln!();
            }
        }
    }

    Ok(())
}

fn resolve_log_target<'a>(template: &StackTemplate, target: &'a str) -> &'a str {
    if matches!(template, StackTemplate::Rust) && target == "wordpress" {
        "app"
    } else {
        target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_default_logs_target_app() {
        assert_eq!(resolve_log_target(&StackTemplate::Rust, "wordpress"), "app");
    }

    #[test]
    fn explicit_logs_target_is_preserved() {
        assert_eq!(
            resolve_log_target(&StackTemplate::Rust, "postgres"),
            "postgres"
        );
        assert_eq!(
            resolve_log_target(&StackTemplate::Wordpress, "wordpress"),
            "wordpress"
        );
    }
}
