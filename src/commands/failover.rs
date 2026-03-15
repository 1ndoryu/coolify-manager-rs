/*
 * failover — restaura un sitio en un VPS alternativo usando backup de Drive.
 *
 * A diferencia de `migrate`, NO requiere conectividad con el VPS origen.
 * Usa el backup mas reciente disponible en Google Drive.
 *
 * Flujo:
 * 1. Buscar backup mas reciente en Drive (daily o weekly)
 * 2. Provisionar stack en VPS destino via Coolify API
 * 3. Restaurar backup (DBs + archivos)
 * 4. Verificar health del sitio
 * 5. Opcionalmente cambiar DNS
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::ssh_client::SshClient;
use crate::services::{backup_manager, dns_manager, health_manager, migration_manager};

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    target_name: &str,
    backup_id: Option<&str>,
    switch_dns: bool,
    skip_provision: bool,
) -> std::result::Result<(), CoolifyError> {
    let mut settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?.clone();
    let target = settings.get_target(target_name)?.clone();

    println!("=== Failover: {} -> {} ===", site.nombre, target.name);

    /* 1. Resolver backup — si no se especifica, buscar el mas reciente en Drive */
    let resolved_backup_id = match backup_id {
        Some(id) => id.to_string(),
        None => find_latest_backup(&settings, config_path, &site.nombre).await?,
    };
    println!("Backup seleccionado: {}", resolved_backup_id);

    /* 2. Provisionar stack en VPS destino (o usar UUID existente) */
    let stack_uuid = if skip_provision {
        site.stack_uuid
            .clone()
            .ok_or_else(|| CoolifyError::Validation(
                "Se requiere stackUuid existente con --skip-provision".to_string(),
            ))?
    } else {
        println!("Provisionando stack en {}...", target.name);
        let api = CoolifyApiClient::new(&target.coolify)?;
        let uuid = migration_manager::provision_target_stack(&settings, &site, &target, &api).await?;
        println!("Stack creado: {}", uuid);
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        uuid
    };

    /* 3. Conectar al VPS destino y restaurar backup */
    let mut target_ssh = SshClient::from_vps(&target.vps);
    target_ssh.connect().await?;

    let mut target_site = site.clone();
    target_site.stack_uuid = Some(stack_uuid.clone());

    println!("Restaurando backup en destino...");
    backup_manager::restore_site_backup(
        &settings,
        config_path,
        &target_site,
        &target_ssh,
        &resolved_backup_id,
        true, /* skip safety snapshot — no hay datos previos en VPS destino */
    )
    .await?;
    println!("Backup restaurado.");

    /* 4. Health check */
    let health = health_manager::assert_site_healthy(&settings, &target_site, &target_ssh).await?;
    println!("Health check: {}", if health.healthy() { "OK" } else { "FALLIDO" });

    /* 5. Actualizar config local — apuntar sitio al nuevo target */
    target_site.target = if target.name == "default" {
        None
    } else {
        Some(target.name.clone())
    };
    target_site.stack_uuid = Some(stack_uuid);
    settings.update_site(target_site.clone(), config_path)?;
    println!("Config actualizada: sitio '{}' ahora apunta a '{}'", site.nombre, target.name);

    /* 6. DNS switch (opcional, respeta switchOnMigration del sitio) */
    let should_switch_dns = switch_dns
        || target_site
            .dns_config
            .as_ref()
            .map(|config| config.switch_on_migration)
            .unwrap_or(false);

    if should_switch_dns {
        let report = dns_manager::switch_site_dns(&settings, &target_site, &target.vps.ip, false).await?;
        println!("DNS actualizado: {} -> {}", report.zone, report.target_ip);
        for action in &report.actions {
            println!("  {} {} -> {} ({})", action.record_type, action.record_name, action.value, action.action);
        }
    } else {
        println!("DNS NO actualizado. Para cambiar: coolify-manager switch-dns --name {} --target-ip {}", site.nombre, target.vps.ip);
    }

    println!("=== Failover completado ===");
    Ok(())
}

/// Busca el backup mas reciente en Drive (prioriza daily sobre weekly).
async fn find_latest_backup(
    settings: &Settings,
    config_path: &Path,
    site_name: &str,
) -> std::result::Result<String, CoolifyError> {
    let entries = backup_manager::list_site_backups(settings, config_path, site_name).await?;
    entries
        .first()
        .map(|entry| entry.backup_id.clone())
        .ok_or_else(|| CoolifyError::Validation(format!(
            "No hay backups disponibles en Google Drive para '{}'", site_name
        )))
}
