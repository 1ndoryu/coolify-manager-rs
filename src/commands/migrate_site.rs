use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::validation;
use crate::services::{dns_manager, migration_manager};

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    target_name: &str,
    dry_run: bool,
    switch_dns: bool,
) -> std::result::Result<(), CoolifyError> {
    let mut settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?.clone();
    validation::assert_site_ready(&site)?;
    let target = settings.get_target(target_name)?.clone();

    let plan =
        migration_manager::migrate_site(&settings, config_path, &site, &target, dry_run).await?;
    println!(
        "Migracion {}: {} -> {} | backup={} | stack={}",
        plan.site_name,
        plan.source_target,
        plan.target,
        plan.backup_id,
        plan.target_stack_uuid
            .clone()
            .unwrap_or_else(|| "pendiente".to_string())
    );
    if dry_run {
        println!("Modo dry-run: preflight completado sin backup ni cambios remotos.");
        for note in &plan.notes {
            println!("- {}", note);
        }
    } else {
        let mut updated_site = site.clone();
        updated_site.target = if target.name == "default" {
            None
        } else {
            Some(target.name.clone())
        };
        updated_site.stack_uuid = plan.target_stack_uuid.clone();
        settings.update_site(updated_site.clone(), config_path)?;
        println!("Health destino: {}", plan.health_ok);
        let should_switch_dns = switch_dns
            || updated_site
                .dns_config
                .as_ref()
                .map(|config| config.switch_on_migration)
                .unwrap_or(false);
        if should_switch_dns {
            let report =
                dns_manager::switch_site_dns(&settings, &updated_site, &target.vps.ip, false)
                    .await?;
            println!(
                "DNS actualizado en {} hacia {}",
                report.zone, report.target_ip
            );
            for action in report.actions {
                println!(
                    "- {} {} -> {} ({})",
                    action.record_type, action.record_name, action.value, action.action
                );
            }
        }
    }
    Ok(())
}
