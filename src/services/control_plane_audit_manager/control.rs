/* Stop/start/status del plano de control (119A-5 split control_plane_audit_manager). */

use super::auditoria::{list_control_plane_containers, list_running_control_plane_containers};
use super::ejecucion::exec_raw_script_timeout;
use super::formato::ok_if_empty;
use crate::config::{DeploymentTargetConfig, VpsConfig};
use crate::error::CoolifyError;

pub async fn stop_control_plane_target(
    target: &DeploymentTargetConfig,
    include_proxy: bool,
) -> std::result::Result<Vec<String>, CoolifyError> {
    change_control_plane_state(&target.name, &target.vps, "stop", include_proxy).await
}

pub async fn start_control_plane_target(
    target: &DeploymentTargetConfig,
    include_proxy: bool,
) -> std::result::Result<Vec<String>, CoolifyError> {
    change_control_plane_state(&target.name, &target.vps, "start", include_proxy).await
}

pub async fn status_control_plane_target(
    target: &DeploymentTargetConfig,
    include_proxy: bool,
) -> std::result::Result<Vec<String>, CoolifyError> {
    change_control_plane_state(&target.name, &target.vps, "status", include_proxy).await
}

pub(super) async fn change_control_plane_state(
    target_name: &str,
    vps: &VpsConfig,
    action: &str,
    include_proxy: bool,
) -> std::result::Result<Vec<String>, CoolifyError> {
    let mut ssh = crate::infra::ssh_client::SshClient::from_vps(vps);
    ssh.connect().await?;

    let containers = list_control_plane_containers(&ssh, include_proxy).await?;
    let running_before = list_running_control_plane_containers(&ssh, include_proxy).await?;
    let mut steps = vec![format!("target={target_name}")];
    steps.push(format!(
        "candidatos={}",
        if containers.is_empty() {
            "none".to_string()
        } else {
            containers.join(",")
        }
    ));
    steps.push(format!(
        "running_before={}",
        if running_before.is_empty() {
            "none".to_string()
        } else {
            running_before.join(",")
        }
    ));

    match action {
        "status" => {}
        "stop" => {
            let to_stop: Vec<String> = containers
                .iter()
                .filter(|name| running_before.iter().any(|running| running == *name))
                .cloned()
                .collect();
            if to_stop.is_empty() {
                steps.push("stop=no-running-control-plane-containers".to_string());
            } else {
                let stop_output = exec_raw_script_timeout(
                    &ssh,
                    &format!("docker stop {} 2>/dev/null || true", to_stop.join(" ")),
                    120,
                )
                .await?;
                steps.push(format!("stop={}", ok_if_empty(&stop_output)));
            }
        }
        "start" => {
            if containers.is_empty() {
                steps.push("start=no-control-plane-containers-found".to_string());
            } else {
                let start_output = exec_raw_script_timeout(
                    &ssh,
                    &format!("docker start {} 2>/dev/null || true", containers.join(" ")),
                    120,
                )
                .await?;
                steps.push(format!("start={}", ok_if_empty(&start_output)));
            }
        }
        _ => unreachable!("accion control-plane no soportada"),
    }

    let running_after = list_running_control_plane_containers(&ssh, include_proxy).await?;
    steps.push(format!(
        "running_after={}",
        if running_after.is_empty() {
            "none".to_string()
        } else {
            running_after.join(",")
        }
    ));

    Ok(steps)
}
