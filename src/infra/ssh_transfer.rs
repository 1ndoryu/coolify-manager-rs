/* Transferencias de archivos sobre SSH (split por dominio desde ssh_client.rs,
 * 299A-1 07-10: respeta limite-lineas/god-object-rs). Metodos `impl SshClient`:
 * el cliente conserva conexion+exec; aqui solo I/O de ficheros. */

use super::ssh_client::SshClient;
use crate::error::{CoolifyError, SshError};
use crate::infra::encoding::{base64_decode, base64_encode};
use crate::infra::ssh_client::CHANNEL_TIMEOUT_SECS;
use crate::infra::ssh_guard;
use russh::ChannelMsg;
use std::path::Path;
use tokio::io::AsyncReadExt;

impl SshClient {
    /// Sube un archivo al servidor remoto via SCP (cat > file).
    pub async fn upload_file(
        &self,
        local_path: &Path,
        remote_path: &str,
    ) -> std::result::Result<(), CoolifyError> {
        let content = std::fs::read(local_path)?;
        let encoded = base64_encode(&content);

        let cmd = format!("echo '{}' | base64 -d > {}", encoded, remote_path);
        let result = self.execute(&cmd).await?;

        if !result.success() {
            return Err(SshError::CommandFailed {
                exit_code: result.exit_code,
                stderr: result.stderr,
            }
            .into());
        }

        Ok(())
    }

    /// Descarga un archivo del servidor remoto.
    pub async fn download_file(
        &self,
        remote_path: &str,
        local_path: &Path,
    ) -> std::result::Result<(), CoolifyError> {
        let cmd = format!("base64 {}", remote_path);
        let result = self.execute(&cmd).await?;

        if !result.success() {
            return Err(SshError::CommandFailed {
                exit_code: result.exit_code,
                stderr: result.stderr,
            }
            .into());
        }

        let decoded = base64_decode(result.stdout.trim())?;
        std::fs::write(local_path, decoded)?;
        Ok(())
    }

    /* [N1] Transferencia eficiente de archivos grandes via SSH channel piping.
     * El metodo base64 (upload_file) falla para archivos >2MB por ARG_MAX del kernel.
     * Este metodo envia bytes crudos por stdin del canal SSH sin base64. */
    pub async fn upload_file_streamed(
        &self,
        local_path: &Path,
        remote_path: &str,
    ) -> std::result::Result<(), CoolifyError> {
        let session = self.session.as_ref().ok_or(SshError::Disconnected)?;

        if let Some(parent) = Path::new(remote_path).parent() {
            let parent_str = parent.display().to_string();
            if !parent_str.is_empty() && parent_str != "/" {
                self.execute(&format!("mkdir -p '{parent_str}'")).await?;
            }
        }

        let mut channel =
            session
                .channel_open_session()
                .await
                .map_err(|e| SshError::ConnectionRefused {
                    host: self.host.clone(),
                    reason: e.to_string(),
                })?;

        /* [299A-1] El `cat >` tambien pasa por el guard: `exec bash -c` conserva
         * el stdin del canal, el streaming no cambia. */
        let cat_inner = format!("cat > '{}'", remote_path);
        let cat_command = if self.usa_guard {
            ssh_guard::envolver(&cat_inner)
        } else {
            cat_inner
        };
        channel
            .exec(true, cat_command)
            .await
            .map_err(|e| SshError::CommandFailed {
                exit_code: -1,
                stderr: e.to_string(),
            })?;

        /* Streamear archivo en chunks de 32KB sin cargarlo completo en RAM.
         * std::fs::read() bloquearia el runtime con archivos de 400+ MB. */
        let mut file = tokio::fs::File::open(local_path).await?;
        let mut buf = vec![0u8; 32768];
        let file_size = tokio::fs::metadata(local_path).await?.len();
        let mut bytes_sent: u64 = 0;
        loop {
            let n = file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            channel
                .data(&buf[..n])
                .await
                .map_err(|e| SshError::CommandFailed {
                    exit_code: -1,
                    stderr: format!("upload_file_streamed data error: {e}"),
                })?;
            bytes_sent += n as u64;
            /* Log de progreso cada 50 MB */
            if bytes_sent % (50 * 1024 * 1024) < n as u64 {
                tracing::info!(
                    "Upload: {:.0}/{:.0} MB ({:.0}%)",
                    bytes_sent as f64 / 1_048_576.0,
                    file_size as f64 / 1_048_576.0,
                    bytes_sent as f64 / file_size as f64 * 100.0
                );
            }
        }

        channel.eof().await.map_err(|e| SshError::CommandFailed {
            exit_code: -1,
            stderr: format!("upload_file_streamed eof error: {e}"),
        })?;

        /* Esperar ExitStatus de cat. Hacemos break en cuanto llega porque `cat > file`
         * no escribe stdout — no hay datos adicionales que esperar despues del exit.
         * Sin el break, el loop quedaría esperando 300s por None (cierre del canal). */
        let mut exit_code = 0i32;
        loop {
            let msg = tokio::time::timeout(
                std::time::Duration::from_secs(CHANNEL_TIMEOUT_SECS),
                channel.wait(),
            )
            .await
            .map_err(|_| SshError::ChannelTimeout {
                seconds: CHANNEL_TIMEOUT_SECS,
            })?;

            match msg {
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    exit_code = exit_status as i32;
                    break; /* [FIX] cat no produce stdout — salir inmediatamente al recibir ExitStatus */
                }
                None => break,
                _ => {}
            }
        }

        if exit_code != 0 {
            return Err(SshError::CommandFailed {
                exit_code,
                stderr: format!("cat failed writing to {remote_path}"),
            }
            .into());
        }

        Ok(())
    }

    /* [N1] Ejecuta comando y retorna stdout como bytes crudos (sin conversion UTF-8).
     * Necesario para descargar archivos binarios sin corrupcion. */
    pub async fn execute_binary(
        &self,
        command: &str,
    ) -> std::result::Result<(Vec<u8>, i32), CoolifyError> {
        let session = self.session.as_ref().ok_or(SshError::Disconnected)?;

        let mut channel =
            session
                .channel_open_session()
                .await
                .map_err(|e| SshError::ConnectionRefused {
                    host: self.host.clone(),
                    reason: e.to_string(),
                })?;

        /* [299A-1] Lecturas (`cat`) tambien envueltas: el guard solo veta la
         * denylist, el contenido viaja igual por stdout. */
        let clean_command = command.replace('\r', "");
        let final_command = if self.usa_guard {
            ssh_guard::envolver(&clean_command)
        } else {
            clean_command
        };
        channel
            .exec(true, final_command)
            .await
            .map_err(|e| SshError::CommandFailed {
                exit_code: -1,
                stderr: e.to_string(),
            })?;

        let mut stdout = Vec::new();
        let mut exit_code = 0i32;

        loop {
            let msg = tokio::time::timeout(
                std::time::Duration::from_secs(CHANNEL_TIMEOUT_SECS),
                channel.wait(),
            )
            .await
            .map_err(|_| SshError::ChannelTimeout {
                seconds: CHANNEL_TIMEOUT_SECS,
            })?;

            match msg {
                Some(ChannelMsg::Data { data }) => {
                    stdout.extend_from_slice(&data);
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    exit_code = exit_status as i32;
                }
                None => break,
                _ => {}
            }
        }

        Ok((stdout, exit_code))
    }

    /* [N1] Descarga archivo binario del servidor remoto via cat.
     * Mas robusto que base64 para archivos grandes. */
    pub async fn download_file_streamed(
        &self,
        remote_path: &str,
        local_path: &Path,
    ) -> std::result::Result<(), CoolifyError> {
        let (data, exit_code) = self.execute_binary(&format!("cat '{remote_path}'")).await?;

        if exit_code != 0 {
            return Err(SshError::CommandFailed {
                exit_code,
                stderr: format!("Failed to read remote file {remote_path}"),
            }
            .into());
        }

        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(local_path, data)?;

        Ok(())
    }
}
