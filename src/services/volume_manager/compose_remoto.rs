/* Subida del compose al host remoto (119A-5 split volume_manager). */

use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) async fn upload_compose_content(
    ssh: &SshClient,
    compose_file: &str,
    content: String,
) -> std::result::Result<(), CoolifyError> {
    let unique_suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    /* [119A-5] canonicalize: temporal generado (pid + nanos), sin input externo. */
    let tmp_name = format!(
        "coolify-manager-compose-{}-{}.yml",
        std::process::id(),
        unique_suffix
    );
    validation::validar_segmento_ruta(&tmp_name, "temporal")?;
    let temp_path = validation::join_segmento_seguro(&std::env::temp_dir(), &tmp_name, "temporal")?;

    std::fs::write(&temp_path, content).map_err(|error| {
        CoolifyError::Validation(format!(
            "No se pudo escribir compose temporal {}: {}",
            temp_path.display(),
            error
        ))
    })?;

    let upload_result = ssh.upload_file(&temp_path, compose_file).await;
    let _ = std::fs::remove_file(&temp_path);
    upload_result
}
