/*
 * Comando: run-script
 * Sube un archivo local al contenedor y lo ejecuta.
 * Resuelve el problema de escaping al pasar scripts complejos via SSH.
 *
 * Flujo: leer archivo local -> base64 -> docker exec: decode + escribir + ejecutar + limpiar
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;

use base64::Engine as _;
use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    script_path: &Path,
    interpreter: Option<&str>,
    target: &str,
    args: Option<&str>,
) -> std::result::Result<(), CoolifyError> {
    /* Validar que el script existe */
    if !script_path.exists() {
        return Err(CoolifyError::Validation(
            format!("Script no encontrado: {}", script_path.display()),
        ));
    }

    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    let stack_uuid = site.stack_uuid.as_deref().unwrap();
    let target_config = settings.resolve_site_target(site)?;

    let mut ssh = SshClient::from_vps(&target_config.vps);
    ssh.connect().await?;

    let container_id = match target {
        "mariadb" => docker::find_mariadb_container(&ssh, stack_uuid).await?,
        "postgres" => docker::find_postgres_container(&ssh, stack_uuid).await?,
        _ => docker::find_wordpress_container(&ssh, stack_uuid).await?,
    };

    /* Detectar interprete por extension si no se especifica */
    let ext = script_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("sh");

    let interpreter = interpreter.unwrap_or(match ext {
        "php" => "php",
        "py" | "python" => "python3",
        _ => "bash",
    });

    /* Leer y codificar el script */
    let script_bytes = std::fs::read(script_path).map_err(|e| {
        CoolifyError::Validation(format!("Error leyendo {}: {}", script_path.display(), e))
    })?;

    let script_b64 = base64::engine::general_purpose::STANDARD.encode(&script_bytes);

    /* Nombre temporal en el contenedor */
    let remote_path = format!("/tmp/cm_script.{}", ext);

    /* Subir, ejecutar, limpiar — todo en un solo docker exec para minimizar roundtrips */
    let args_str = args.unwrap_or("");
    let full_cmd = format!(
        "echo '{}' | base64 -d > {} && {} {} {} 2>&1; EXIT_CODE=$?; rm -f {}; exit $EXIT_CODE",
        script_b64, remote_path, interpreter, remote_path, args_str, remote_path
    );

    tracing::info!(
        "Ejecutando {} ({} bytes) con {} en {}",
        script_path.display(),
        script_bytes.len(),
        interpreter,
        container_id
    );

    let result = docker::docker_exec(&ssh, &container_id, &full_cmd).await?;

    if !result.stdout.is_empty() {
        print!("{}", result.stdout);
    }
    if !result.stderr.is_empty() {
        eprint!("{}", result.stderr);
    }

    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("Script fallo con exit code {}", result.exit_code),
        });
    }

    Ok(())
}
