/* Sync de envs runtime en el compose (119A-5 split volume_manager). */

use super::compose_envs::upsert_service_environment_entries;
use super::compose_remoto::upload_compose_content;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

/* [155A-11] Coolify puede guardar envs nuevas en su API pero dejar el
 * docker-compose.yml en disco sin esas claves. Como deploy-service recrea el
 * contenedor con docker compose directo sobre ese archivo, el runtime puede
 * quedar atrasado aunque sync-env diga "sincronizado". Antes del swap,
 * insertar en el compose efectivo las envs runtime faltantes de app. */
pub async fn ensure_runtime_envs_in_compose(
    ssh: &SshClient,
    service_dir: &str,
    app_service_name: &str,
    runtime_envs: &[(String, String)],
) -> std::result::Result<(), CoolifyError> {
    if runtime_envs.is_empty() {
        return Ok(());
    }

    let compose_file = format!("{}/docker-compose.yml", service_dir);
    let compose = ssh.execute(&format!("cat {}", compose_file)).await?;
    if !compose.success() || compose.stdout.trim().is_empty() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo leer {} para sincronizar envs runtime",
            compose_file
        )));
    }

    let sync = upsert_service_environment_entries(&compose.stdout, app_service_name, runtime_envs)?;
    if sync.inserted_keys.is_empty() && sync.updated_keys.is_empty() {
        return Ok(());
    }

    upload_compose_content(ssh, &compose_file, sync.content).await?;

    if !sync.inserted_keys.is_empty() {
        println!(
            "      Compose envs runtime sincronizadas: {}",
            sync.inserted_keys.join(", ")
        );
    }
    if !sync.updated_keys.is_empty() {
        println!(
            "      Compose envs runtime actualizadas: {}",
            sync.updated_keys.join(", ")
        );
    }

    Ok(())
}
