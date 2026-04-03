use crate::config::{DeploymentTargetConfig, Settings};
use crate::domain::SiteConfig;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;
use crate::infra::template_engine;
use crate::services::{backup_manager, database_manager, health_manager, site_capabilities};

use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct MigrationPlan {
    pub site_name: String,
    pub source_target: String,
    pub target: String,
    pub backup_id: String,
    pub target_stack_name: String,
    pub target_stack_uuid: Option<String>,
    pub health_ok: bool,
    pub notes: Vec<String>,
}

pub async fn migrate_site(
    settings: &Settings,
    config_path: &Path,
    site: &SiteConfig,
    target: &DeploymentTargetConfig,
    dry_run: bool,
) -> std::result::Result<MigrationPlan, CoolifyError> {
    let source_target = settings.resolve_site_target(site)?;
    let mut source_ssh = SshClient::from_vps(&source_target.vps);
    source_ssh.connect().await?;

    if dry_run {
        return build_dry_run_plan(site, &source_target, target, &source_ssh).await;
    }

    let backup = backup_manager::create_site_backup(
        settings,
        config_path,
        site,
        &source_ssh,
        crate::domain::BackupTier::Manual,
        Some("migration-source"),
    )
    .await?;

    let api = CoolifyApiClient::new(&target.coolify)?;
    let stack_uuid = provision_target_stack(settings, site, target, &api).await?;

    tokio::time::sleep(std::time::Duration::from_secs(30)).await;

    let mut target_ssh = SshClient::from_vps(&target.vps);
    target_ssh.connect().await?;

    let mut target_site = site.clone();
    target_site.stack_uuid = Some(stack_uuid.clone());

    backup_manager::restore_site_backup(
        settings,
        config_path,
        &target_site,
        &target_ssh,
        &backup.backup_id,
        false,
    )
    .await?;
    let health = health_manager::assert_site_healthy(settings, &target_site, &target_ssh).await?;

    Ok(MigrationPlan {
        site_name: site.nombre.clone(),
        source_target: source_target.name,
        target: target.name.clone(),
        backup_id: backup.backup_id,
        target_stack_name: site.nombre.clone(),
        target_stack_uuid: Some(stack_uuid),
        health_ok: health.healthy(),
        notes: Vec::new(),
    })
}

async fn build_dry_run_plan(
    site: &SiteConfig,
    source_target: &DeploymentTargetConfig,
    target: &DeploymentTargetConfig,
    source_ssh: &SshClient,
) -> std::result::Result<MigrationPlan, CoolifyError> {
    let caps = site_capabilities::resolve(site);
    let stack_uuid = site.stack_uuid.as_deref().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{}' sin stackUuid", site.nombre))
    })?;
    let mut notes = Vec::new();

    let app_container = caps.resolve_app_container(source_ssh, stack_uuid).await?;
    notes.push(format!(
        "Origen accesible: contenedor app '{}' resuelto",
        app_container
    ));

    for binding in &caps.database_bindings {
        let db_container = caps
            .resolve_database_container(source_ssh, stack_uuid, binding)
            .await?;
        match binding.engine {
            crate::domain::DatabaseEngine::Mariadb => {
                let (db_name, db_user, _) =
                    database_manager::resolve_wordpress_credentials(source_ssh, &app_container)
                        .await?;
                notes.push(format!(
                    "BD '{}' valida: contenedor='{}' db='{}' user='{}'",
                    binding.logical_name, db_container, db_name, db_user
                ));
            }
            crate::domain::DatabaseEngine::Postgres => {
                let (db_name, db_user, _) =
                    database_manager::resolve_postgres_credentials(source_ssh, &app_container)
                        .await?;
                notes.push(format!(
                    "BD '{}' valida: contenedor='{}' db='{}' user='{}'",
                    binding.logical_name, db_container, db_name, db_user
                ));
            }
        }
    }

    for source_path in &caps.persistent_paths {
        validate_container_path_exists(source_ssh, &app_container, source_path).await?;
        notes.push(format!("Ruta persistente valida: {}", source_path));
    }

    let mut target_ssh = SshClient::from_vps(&target.vps);
    target_ssh.connect().await?;
    notes.push(format!(
        "Destino SSH accesible: {}@{}",
        target.vps.user, target.vps.ip
    ));

    let missing_fields = missing_target_coolify_fields(target);
    if missing_fields.is_empty() {
        notes.push(format!(
            "Coolify destino listo: server='{}' project='{}'",
            target.coolify.server_uuid, target.coolify.project_uuid
        ));
    } else {
        notes.push(format!(
            "Coolify destino incompleto para migracion real: faltan {}",
            missing_fields.join(", ")
        ));
    }

    Ok(MigrationPlan {
        site_name: site.nombre.clone(),
        source_target: source_target.name.clone(),
        target: target.name.clone(),
        backup_id: "preflight-sin-backup".to_string(),
        target_stack_name: site.nombre.clone(),
        target_stack_uuid: None,
        health_ok: false,
        notes,
    })
}

async fn validate_container_path_exists(
    ssh: &SshClient,
    container_id: &str,
    source_path: &str,
) -> std::result::Result<(), CoolifyError> {
    let command = format!("test -e {}", shell_quote(source_path));
    let result = docker::docker_exec(ssh, container_id, &command).await?;
    if result.success() {
        return Ok(());
    }

    Err(CoolifyError::Validation(format!(
        "Ruta persistente no disponible para preflight: {}",
        source_path
    )))
}

fn missing_target_coolify_fields(target: &DeploymentTargetConfig) -> Vec<&'static str> {
    let mut missing = Vec::new();

    if target.coolify.api_token.trim().is_empty() {
        missing.push("apiToken");
    }
    if target.coolify.server_uuid.trim().is_empty() {
        missing.push("serverUuid");
    }
    if target.coolify.project_uuid.trim().is_empty() {
        missing.push("projectUuid");
    }

    missing
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub async fn provision_target_stack(
    settings: &Settings,
    site: &SiteConfig,
    target: &DeploymentTargetConfig,
    api: &CoolifyApiClient,
) -> std::result::Result<String, CoolifyError> {
    let compose_yaml = build_compose_for_site(settings, site)?;
    let result = api
        .create_stack(
            &site.nombre,
            &target.coolify.server_uuid,
            &target.coolify.project_uuid,
            &target.coolify.environment_name,
            &compose_yaml,
        )
        .await?;
    Ok(result.uuid)
}

pub fn build_compose_for_site(
    _settings: &Settings,
    site: &SiteConfig,
) -> std::result::Result<String, CoolifyError> {
    let db_password = template_engine::generate_password(24);
    let root_password = template_engine::generate_password(24);
    let vars = match site.template {
        crate::domain::StackTemplate::Wordpress => {
            template_engine::wordpress_vars(&site.dominio, &db_password, &root_password)
        }
        crate::domain::StackTemplate::Kamples => {
            let pg_password = template_engine::generate_password(24);
            template_engine::kamples_vars(
                &site.dominio,
                &db_password,
                &root_password,
                &pg_password,
                &site.glory_branch,
            )
        }
        crate::domain::StackTemplate::Minecraft => template_engine::minecraft_vars(&site.nombre),
        crate::domain::StackTemplate::Rust => {
            let repo_url = site.repo_url.as_deref()
                .unwrap_or("https://github.com/1ndoryu/glory-rs.git");
            template_engine::rust_vars(
                &site.dominio,
                &site.glory_branch,
                repo_url,
            )
        }
    };

    let template_file =
        std::path::Path::new("templates").join(format!("{}-stack.yaml", site.template));
    if template_file.exists() {
        return template_engine::render_file(&template_file, &vars);
    }

    let caps = site_capabilities::resolve(site);
    Err(CoolifyError::Template(format!(
        "Template no encontrado para '{}' (contenedor app esperado: {})",
        site.template, caps.app_name_hint
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CoolifyConfig, VpsConfig};

    #[test]
    fn test_missing_target_coolify_fields_reports_only_empty_values() {
        let target = DeploymentTargetConfig {
            name: "standby".to_string(),
            vps: VpsConfig {
                ip: "1.2.3.4".to_string(),
                user: "root".to_string(),
                ssh_key: None,
                ssh_password: None,
            },
            coolify: CoolifyConfig {
                base_url: "http://coolify.local".to_string(),
                api_token: String::new(),
                server_uuid: "srv-1".to_string(),
                project_uuid: String::new(),
                environment_name: "production".to_string(),
            },
        };

        assert_eq!(
            missing_target_coolify_fields(&target),
            vec!["apiToken", "projectUuid"]
        );
    }

    #[test]
    fn test_shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("/tmp/o'hara"), "'/tmp/o'\\''hara'");
    }
}
