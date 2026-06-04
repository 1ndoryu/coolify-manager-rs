use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::control_plane_audit_manager;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    target_name: &str,
    action: &str,
    include_proxy: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let target = settings.get_target(target_name)?;
    let lines = match action.trim().to_ascii_lowercase().as_str() {
        "stop" => {
            control_plane_audit_manager::stop_control_plane_target(target, include_proxy).await?
        }
        "start" => {
            control_plane_audit_manager::start_control_plane_target(target, include_proxy).await?
        }
        "status" => {
            control_plane_audit_manager::status_control_plane_target(target, include_proxy).await?
        }
        other => {
            return Err(CoolifyError::Validation(format!(
                "Accion invalida para coolify-control-plane: {other}. Usa stop, start o status."
            )));
        }
    };

    for line in lines {
        println!("{line}");
    }

    Ok(())
}
