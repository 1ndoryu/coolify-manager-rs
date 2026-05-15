/*
 * Comando: view-logs
 * Obtiene logs del contenedor o debug.log de WordPress.
 */

use crate::config::Settings;
use crate::domain::StackTemplate;
use crate::error::CoolifyError;
use crate::infra::docker;
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
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    let stack_uuid = site.stack_uuid.as_deref().unwrap();
    let target_config = settings.resolve_site_target(site)?;

    let mut ssh = SshClient::from_vps(&target_config.vps);
    ssh.connect().await?;

    let effective_target = resolve_log_target(&site.template, target);

    if wp_debug && effective_target != "wordpress" {
        return Err(CoolifyError::Validation(
            "--wp-debug solo aplica a stacks WordPress".to_string(),
        ));
    }

    /* [114A-6] Soporte para target 'app' y 'websocket' en logs */
    let container_id = match effective_target {
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
