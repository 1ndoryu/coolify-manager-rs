/* Reparación del plano de control (119A-5 split control_plane_audit_manager). */

use super::auditoria::inspect_proxy_network_drift;
use super::ejecucion::exec_raw_script_timeout;
use super::formato::{empty_as_unknown, format_name_list, ok_if_empty, sh_quote};
use super::tipos::COOLIFY_PROXY_CONTAINER;
use crate::config::{DeploymentTargetConfig, Settings, VpsConfig};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

pub async fn repair_default_vps(
    settings: &Settings,
) -> std::result::Result<Vec<String>, CoolifyError> {
    repair_vps_config("default", &settings.vps).await
}

pub async fn repair_target(
    target: &DeploymentTargetConfig,
) -> std::result::Result<Vec<String>, CoolifyError> {
    repair_vps_config(&target.name, &target.vps).await
}

pub(super) async fn repair_vps_config(
    target_name: &str,
    vps: &VpsConfig,
) -> std::result::Result<Vec<String>, CoolifyError> {
    let mut ssh = SshClient::from_vps(vps);
    ssh.connect().await?;

    let artisan_commands = exec_raw_script_timeout(
        &ssh,
        r#"docker exec coolify php artisan list --raw 2>/dev/null || true"#,
        60,
    )
    .await?;
    let supports_clear_metrics = artisan_commands
        .lines()
        .any(|line| line.trim() == "horizon:clear-metrics");
    let supports_schedule_clear_cache = artisan_commands
        .lines()
        .any(|line| line.trim() == "schedule:clear-cache");

    let failed_before = exec_raw_script_timeout(
        &ssh,
        r#"docker exec coolify sh -lc 'failed_lines=$(php artisan queue:failed 2>/dev/null | sed "/There are no failed jobs/d;/^[[:space:]]*$/d"); if [ -n "$failed_lines" ]; then echo "$failed_lines" | tail -n +2 | wc -l | tr -d " "; else echo 0; fi' 2>/dev/null || echo unknown"#,
        60,
    )
    .await?;

    let mut steps = vec![format!("target={target_name}")];
    steps.push(format!(
        "failed_jobs_before={}",
        empty_as_unknown(&failed_before)
    ));

    sync_proxy_network_drift(&ssh, &mut steps).await?;

    let queue_flush = exec_raw_script_timeout(
        &ssh,
        r#"docker exec coolify php artisan queue:flush --no-interaction 2>/dev/null || echo queue-flush-unavailable"#,
        120,
    )
    .await?;
    steps.push(format!("queue_flush={}", ok_if_empty(&queue_flush)));

    if supports_clear_metrics {
        let clear_metrics = exec_raw_script_timeout(
            &ssh,
            r#"docker exec coolify php artisan horizon:clear-metrics --no-interaction 2>/dev/null || echo horizon-clear-metrics-failed"#,
            120,
        )
        .await?;
        steps.push(format!(
            "horizon_clear_metrics={}",
            ok_if_empty(&clear_metrics)
        ));
    } else {
        steps.push("horizon_clear_metrics=unsupported".to_string());
    }

    if supports_schedule_clear_cache {
        let clear_schedule_cache = exec_raw_script_timeout(
            &ssh,
            r#"docker exec coolify php artisan schedule:clear-cache --no-interaction 2>/dev/null || echo schedule-clear-cache-failed"#,
            60,
        )
        .await?;
        steps.push(format!(
            "schedule_clear_cache={}",
            ok_if_empty(&clear_schedule_cache)
        ));
    }

    let terminate_horizon = exec_raw_script_timeout(
        &ssh,
        r#"docker exec coolify php artisan horizon:terminate --no-interaction 2>/dev/null || echo horizon-terminate-failed"#,
        60,
    )
    .await?;
    steps.push(format!(
        "horizon_terminate={}",
        ok_if_empty(&terminate_horizon)
    ));

    let failed_after = exec_raw_script_timeout(
        &ssh,
        r#"docker exec coolify sh -lc 'failed_lines=$(php artisan queue:failed 2>/dev/null | sed "/There are no failed jobs/d;/^[[:space:]]*$/d"); if [ -n "$failed_lines" ]; then echo "$failed_lines" | tail -n +2 | wc -l | tr -d " "; else echo 0; fi' 2>/dev/null || echo unknown"#,
        60,
    )
    .await?;
    steps.push(format!(
        "failed_jobs_after={}",
        empty_as_unknown(&failed_after)
    ));

    Ok(steps)
}

pub(super) async fn sync_proxy_network_drift(
    ssh: &SshClient,
    steps: &mut Vec<String>,
) -> std::result::Result<(), CoolifyError> {
    let proxy_network_drift_before = inspect_proxy_network_drift(ssh).await?;
    steps.push(format!(
        "proxy_networks_before={}",
        format_name_list(&proxy_network_drift_before.proxy_networks)
    ));
    steps.push(format!(
        "workload_networks={}",
        format_name_list(&proxy_network_drift_before.workload_networks)
    ));
    steps.push(format!(
        "proxy_networks_missing_before={}",
        format_name_list(&proxy_network_drift_before.missing_networks)
    ));

    if proxy_network_drift_before.missing_networks.is_empty() {
        steps.push("proxy_network_sync=already-aligned".to_string());
    } else {
        for network in &proxy_network_drift_before.missing_networks {
            let connect_output = exec_raw_script_timeout(
                ssh,
                &format!(
                    "docker network connect {} {} 2>&1 || true",
                    sh_quote(network),
                    COOLIFY_PROXY_CONTAINER
                ),
                60,
            )
            .await?;
            steps.push(format!(
                "proxy_network_connect[{network}]={}",
                ok_if_empty(&connect_output)
            ));
        }
    }

    let proxy_network_drift_after = inspect_proxy_network_drift(ssh).await?;
    steps.push(format!(
        "proxy_networks_missing_after={}",
        format_name_list(&proxy_network_drift_after.missing_networks)
    ));

    Ok(())
}
