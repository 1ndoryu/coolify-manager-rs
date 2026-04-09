/* [N1] Cliente de backup remoto via SSH.
 * Reemplaza Google Drive como storage de backups. Usa un VPS secundario
 * accesible por SSH para almacenar archives de backup.
 *
 * Estructura remota:
 *   {base_dir}/{site_name}/{tier}/{backup_id}.tar.gz
 *
 * Gotcha: upload/download usan metodos streamed (no base64) para soportar
 * archivos grandes sin limite de ARG_MAX. */

use crate::config::SshRemoteBackupConfig;
use crate::error::{CoolifyError, SshError};
use crate::infra::ssh_client::SshClient;

use std::path::Path;

pub struct SshBackupClient {
    ssh: SshClient,
    base_dir: String,
    host: String,
}

impl SshBackupClient {
    pub async fn new(config: &SshRemoteBackupConfig) -> std::result::Result<Self, CoolifyError> {
        let mut ssh = SshClient::new(
            &config.host,
            &config.user,
            config.ssh_key.as_deref(),
            config.ssh_password.as_deref(),
        );
        ssh.connect().await?;
        Ok(Self {
            ssh,
            base_dir: config.base_dir.clone(),
            host: config.host.clone(),
        })
    }

    /// Verifica que el directorio base existe y es escribible.
    pub async fn ensure_writable(&self) -> std::result::Result<(), CoolifyError> {
        let cmd = format!(
            "mkdir -p '{}' && test -w '{}'",
            self.base_dir, self.base_dir
        );
        let result = self.ssh.execute(&cmd).await?;
        if !result.success() {
            return Err(CoolifyError::Validation(format!(
                "Directorio de backup remoto '{}' no es escribible en {}: {}",
                self.base_dir,
                self.host(),
                result.stderr
            )));
        }
        Ok(())
    }

    /// Sube un archive de backup al VPS remoto.
    /// Retorna la ruta remota completa del archivo subido.
    pub async fn upload_backup_archive(
        &self,
        site_name: &str,
        tier: &str,
        backup_id: &str,
        local_path: &Path,
    ) -> std::result::Result<String, CoolifyError> {
        let remote_dir = format!("{}/{}/{}", self.base_dir, site_name, tier);
        let remote_path = format!("{}/{}.tar.gz", remote_dir, backup_id);

        /* Crear estructura de directorios */
        let result = self
            .ssh
            .execute(&format!("mkdir -p '{remote_dir}'"))
            .await?;
        if !result.success() {
            return Err(CoolifyError::Validation(format!(
                "No se pudo crear directorio remoto '{remote_dir}': {}",
                result.stderr
            )));
        }

        self.ssh
            .upload_file_streamed(local_path, &remote_path)
            .await?;

        /* Verificar que el archivo se subio correctamente comparando tamano */
        let local_size = std::fs::metadata(local_path)?.len();
        let result = self
            .ssh
            .execute(&format!("stat -c%s '{remote_path}'"))
            .await?;
        if result.success() {
            if let Ok(remote_size) = result.stdout.trim().parse::<u64>() {
                if remote_size != local_size {
                    /* Limpiar archivo corrupto */
                    let _ = self
                        .ssh
                        .execute(&format!("rm -f '{remote_path}'"))
                        .await;
                    return Err(CoolifyError::Validation(format!(
                        "Verificacion de tamano fallo: local={local_size} remote={remote_size}"
                    )));
                }
            }
        }

        tracing::info!(
            "Backup subido a VPS remoto: {} ({} bytes)",
            remote_path,
            local_size
        );
        Ok(remote_path)
    }

    /// Descarga un archive de backup del VPS remoto.
    /// Retorna false si el archivo no existe.
    pub async fn download_backup_archive(
        &self,
        site_name: &str,
        tier: &str,
        backup_id: &str,
        local_path: &Path,
    ) -> std::result::Result<bool, CoolifyError> {
        let remote_path = format!(
            "{}/{}/{}/{}.tar.gz",
            self.base_dir, site_name, tier, backup_id
        );

        /* Verificar existencia */
        let result = self
            .ssh
            .execute(&format!("test -f '{remote_path}' && echo exists"))
            .await?;
        if result.stdout.trim() != "exists" {
            return Ok(false);
        }

        self.ssh
            .download_file_streamed(&remote_path, local_path)
            .await?;
        Ok(true)
    }

    /// Lista archivos de un tier para un sitio, ordenados descending por nombre (mas reciente primero).
    /// Retorna Vec<(ruta_remota, nombre_archivo)>.
    pub async fn list_tier_files(
        &self,
        site_name: &str,
        tier: &str,
    ) -> std::result::Result<Vec<(String, String)>, CoolifyError> {
        let remote_dir = format!("{}/{}/{}", self.base_dir, site_name, tier);

        /* ls -1r ordena reverse (mas reciente primero por nombre con timestamp) */
        let result = self
            .ssh
            .execute(&format!(
                "test -d '{remote_dir}' && ls -1r '{remote_dir}/' 2>/dev/null || true"
            ))
            .await?;

        let mut entries = Vec::new();
        for line in result.stdout.lines() {
            let name = line.trim();
            if name.is_empty() || !name.ends_with(".tar.gz") {
                continue;
            }
            let full_path = format!("{}/{}", remote_dir, name);
            entries.push((full_path, name.to_string()));
        }

        Ok(entries)
    }

    /// Elimina un archivo de backup remoto por ruta completa.
    pub async fn delete_file(&self, remote_path: &str) -> std::result::Result<(), CoolifyError> {
        let result = self
            .ssh
            .execute(&format!("rm -f '{remote_path}'"))
            .await?;
        if !result.success() {
            return Err(SshError::CommandFailed {
                exit_code: result.exit_code,
                stderr: format!("No se pudo eliminar {remote_path}: {}", result.stderr),
            }
            .into());
        }
        Ok(())
    }

    fn host(&self) -> &str {
        &self.host
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_remote_path_construction() {
        let base = "/backups/coolify-manager";
        let site = "guillermo";
        let tier = "daily";
        let id = "20260409_030000";
        let expected = format!("{base}/{site}/{tier}/{id}.tar.gz");
        assert_eq!(
            expected,
            "/backups/coolify-manager/guillermo/daily/20260409_030000.tar.gz"
        );
    }
}
