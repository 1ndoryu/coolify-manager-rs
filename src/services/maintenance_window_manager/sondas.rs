/* Split 119A-5 de maintenance_window_manager.rs — ejecutores y parseo base.
 * Código verbatim del original; solo cambia visibilidad a pub(super). */

use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;

pub(super) async fn exec_trim(
    ssh: &SshClient,
    command: &str,
) -> std::result::Result<String, CoolifyError> {
    let result = ssh.execute(command).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo ejecutando comando remoto: {}{}",
            result.stdout, result.stderr
        )));
    }
    Ok(result.stdout.trim().to_string())
}

pub(super) async fn upload_remote_text(
    ssh: &SshClient,
    content: &str,
    remote_path: &str,
) -> std::result::Result<(), CoolifyError> {
    /* [119A-5] canonicalize: temporal generado (pid + uuid v4), sin input externo. */
    let tmp_name = format!(
        "coolify-manager-{}-{}.tmp",
        std::process::id(),
        uuid::Uuid::new_v4()
    );
    validation::validar_segmento_ruta(&tmp_name, "temporal")?;
    let temp_path = validation::join_segmento_seguro(&std::env::temp_dir(), &tmp_name, "temporal")?;
    std::fs::write(&temp_path, content).map_err(|error| {
        CoolifyError::Validation(format!("No se pudo crear archivo temporal: {error}"))
    })?;
    let upload_result = ssh.upload_file(&temp_path, remote_path).await;
    let _ = std::fs::remove_file(&temp_path);
    upload_result
}

pub(super) fn parse_f32(value: &str, fallback: f32) -> f32 {
    value.trim().parse::<f32>().unwrap_or(fallback)
}

pub(super) fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) fn empty_as_unknown(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}
