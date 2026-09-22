/* Bind mount SSH runtime en el compose (119A-5 split volume_manager). */

use super::compose_envs::upsert_service_environment_entries;
use super::compose_remoto::upload_compose_content;
use super::compose_volumenes::ensure_service_volume_entry;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

/* [235A-6] Las rutas SSH guardadas en Coolify pueden venir del equipo local
 * Windows y no existir dentro del contenedor. Si el host tiene una clave
 * `/root/{site}-ssh/id_ed25519`, el compose efectivo monta esa carpeta y usa
 * la ruta Linux esperada por el sampler de infraestructura. */
pub async fn ensure_runtime_ssh_bind_mount(
    ssh: &SshClient,
    service_dir: &str,
    app_service_name: &str,
    site_name: &str,
) -> std::result::Result<(), CoolifyError> {
    let host_ssh_dir = format!("/root/{}-ssh", site_name);
    let host_key_path = format!("{host_ssh_dir}/id_ed25519");
    let key_check = ssh
        .execute(&format!(
            "test -r '{}' && echo PRESENT || echo MISSING",
            host_key_path
        ))
        .await?;
    if !key_check.stdout.contains("PRESENT") {
        return Ok(());
    }

    let vps2_key_path = format!("{host_ssh_dir}/vps2_backup");
    let vps2_key_sync = ssh
        .execute(&format!(
            "mkdir -p '{host_ssh_dir}' && if test -r /root/.ssh/vps2_backup; then cp /root/.ssh/vps2_backup '{vps2_key_path}' && chmod 600 '{vps2_key_path}' && echo VPS2_PRESENT; else echo VPS2_MISSING; fi"
        ))
        .await?;
    let has_vps2_key = vps2_key_sync.stdout.contains("VPS2_PRESENT");

    let compose_file = format!("{}/docker-compose.yml", service_dir);
    let compose = ssh.execute(&format!("cat {}", compose_file)).await?;
    if !compose.success() || compose.stdout.trim().is_empty() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo leer {} para sincronizar mount SSH",
            compose_file
        )));
    }

    let volume_entry = format!("{}:/home/appuser/.ssh", host_ssh_dir);
    let volume_sync =
        ensure_service_volume_entry(&compose.stdout, app_service_name, &volume_entry)?;
    let mut ssh_envs = vec![(
        "COOLIFY_VPS1_SSH_KEY_PATH".to_string(),
        "/home/appuser/.ssh/id_ed25519".to_string(),
    )];
    if has_vps2_key {
        ssh_envs.push((
            "COOLIFY_SSH_KEY_PATH".to_string(),
            "/home/appuser/.ssh/vps2_backup".to_string(),
        ));
    }
    let env_sync =
        upsert_service_environment_entries(&volume_sync.content, app_service_name, &ssh_envs)?;

    if !volume_sync.changed && env_sync.inserted_keys.is_empty() && env_sync.updated_keys.is_empty()
    {
        return Ok(());
    }

    upload_compose_content(ssh, &compose_file, env_sync.content).await?;
    if volume_sync.changed {
        println!("      Compose mount SSH sincronizado: {volume_entry}");
    }
    if !env_sync.inserted_keys.is_empty() || !env_sync.updated_keys.is_empty() {
        let mut changed_keys = env_sync.inserted_keys;
        changed_keys.extend(env_sync.updated_keys);
        println!(
            "      Compose env SSH sincronizada: {}",
            changed_keys.join(", ")
        );
    }
    Ok(())
}
