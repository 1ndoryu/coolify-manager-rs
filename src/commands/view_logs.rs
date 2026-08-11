/*
 * Comando: view-logs
 * Obtiene logs del contenedor o debug.log de WordPress.
 *
 * [257B-1] Soporte para --since, --until, --pattern para búsqueda temporal.
 */

use crate::commands::container::resolve_relative_time;
use crate::config::Settings;
use crate::domain::StackTemplate;
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::docker_api::DockerApiClient;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;

use std::path::Path;

/* [257B-1] Los parámetros opcionales since/until/pattern aumentan el conteo de args.
 * Esta función es un dispatcher CLI que mapea 1:1 con los flags de clap. */
#[allow(clippy::too_many_arguments)]
pub async fn execute(
    config_path: &Path,
    site_name: &str,
    lines: u32,
    target: &str,
    wp_debug: bool,
    filter: Option<&str>,
    docker_socket: Option<&str>,
    since: Option<&str>,
    until: Option<&str>,
    pattern: Option<&str>,
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

    /* Combinar filter y pattern en un solo patrón de búsqueda */
    let search_pattern = match (filter, pattern) {
        (Some(f), Some(p)) => Some(format!("{}|{}", f, p)),
        (Some(f), None) => Some(f.to_string()),
        (None, Some(p)) => Some(p.to_string()),
        (None, None) => None,
    };

    /* Modo Docker API: sin SSH, conecta directo al daemon */
    if let Some(socket) = docker_socket {
        return execute_via_docker_api(
            socket,
            stack_uuid,
            effective_target,
            lines,
            search_pattern.as_deref(),
        )
        .await;
    }

    /* Modo SSH (comportamiento original) */
    let target_config = settings.resolve_site_target(site)?;
    let mut ssh = SshClient::from_vps(&target_config.vps);
    ssh.connect().await?;

    let container_id = match effective_target {
        "site" => docker::find_site_container(&ssh, stack_uuid).await?,
        "mariadb" => docker::find_mariadb_container(&ssh, stack_uuid).await?,
        "postgres" => docker::find_postgres_container(&ssh, stack_uuid).await?,
        "app" => docker::find_app_container(&ssh, stack_uuid).await?,
        "websocket" => docker::find_websocket_container(&ssh, stack_uuid).await?,
        _ => docker::find_wordpress_container(&ssh, stack_uuid).await?,
    };

    let output = if wp_debug {
        let mut cmd =
            format!("cat /var/www/html/wp-content/debug.log 2>/dev/null | tail -n {lines}");
        if let Some(pat) = &search_pattern {
            /* [257B-1] Usar grep -iF para strings fijos y -iE para regex compuesto */
            cmd = format!(
                "cat /var/www/html/wp-content/debug.log 2>/dev/null | grep -iF '{}' | tail -n {lines}",
                pat.replace('\'', "'\\''")
            );
        }
        docker::docker_exec(&ssh, &container_id, &cmd).await?
    } else {
        /* [257B-1] Construir docker logs con soporte temporal */
        let mut cmd = format!("docker logs --tail {lines}");
        if let Some(s) = since {
            cmd.push_str(&format!(" --since '{}'", resolve_relative_time(s)));
        }
        if let Some(u) = until {
            cmd.push_str(&format!(" --until '{}'", resolve_relative_time(u)));
        }
        cmd.push_str(&format!(" {container_id} 2>&1"));
        ssh.execute(&cmd).await?
    };

    /* Aplicar filtro de patrón post-obtención si hay since/until + pattern */
    let final_output = if let Some(pat) = &search_pattern {
        let filtered_stdout: Vec<&str> = output
            .stdout
            .lines()
            .filter(|line| line.to_lowercase().contains(&pat.to_lowercase()))
            .collect();
        let filtered_stderr: Vec<&str> = output
            .stderr
            .lines()
            .filter(|line| line.to_lowercase().contains(&pat.to_lowercase()))
            .collect();
        if filtered_stdout.is_empty() && filtered_stderr.is_empty() {
            println!("(sin logs que coincidan con '{}')", pat);
            return Ok(());
        }
        if !filtered_stdout.is_empty() {
            for line in &filtered_stdout {
                println!("{}", line);
            }
        }
        if !filtered_stderr.is_empty() {
            for line in &filtered_stderr {
                eprintln!("{}", line);
            }
        }
        return Ok(());
    } else {
        output
    };

    if final_output.stdout.is_empty() && final_output.stderr.is_empty() {
        println!("(sin logs disponibles)");
    } else {
        if !final_output.stdout.is_empty() {
            print!("{}", final_output.stdout);
        }
        if !final_output.stderr.is_empty() {
            eprint!("{}", final_output.stderr);
        }
    }

    Ok(())
}

/// Obtiene logs usando el Docker Engine API directamente (sin SSH).
async fn execute_via_docker_api(
    socket: &str,
    stack_uuid: &str,
    target: &str,
    lines: u32,
    filter: Option<&str>,
) -> std::result::Result<(), CoolifyError> {
    let client = DockerApiClient::connect(Some(socket))?;

    client.ping().await?;

    let name_hint = match target {
        "site" => stack_uuid.to_string(),
        "mariadb" => format!("{stack_uuid}-mariadb"),
        "postgres" => format!("{stack_uuid}-postgres"),
        "app" => format!("{stack_uuid}-app"),
        "websocket" => format!("{stack_uuid}-websocket"),
        _ => format!("{stack_uuid}-wordpress"),
    };

    let container_name = match client.resolve_container_name(&name_hint).await {
        Ok(name) => name,
        Err(_) => {
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
