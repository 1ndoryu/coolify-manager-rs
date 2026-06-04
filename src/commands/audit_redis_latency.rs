use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::redis_latency_manager;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    target_name: Option<&str>,
    slowlog_count: u16,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;

    let report = match target_name {
        Some(name) => {
            let target = settings.get_target(name)?;
            redis_latency_manager::audit_target(target, slowlog_count).await?
        }
        None => redis_latency_manager::audit_default_vps(&settings, slowlog_count).await?,
    };

    println!("Target: {}", report.target);
    println!("Redis ping: {}", report.redis_ping);
    println!("THP: {}", report.thp_summary);
    println!("Sysctl: {}", report.sysctl_summary);
    println!("Latency: {}", report.latency_summary);
    println!("Slowlog: {}", report.slowlog_summary);
    println!("Redis info: {}", report.redis_info_summary);
    for recommendation in report.recommendations {
        println!("- {recommendation}");
    }

    Ok(())
}
