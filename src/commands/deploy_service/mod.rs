/*
 * [044A-1] Comando: deploy-service
 * Deploy zero-downtime para servicios Docker Compose gestionados por Coolify.
 * Construye la imagen nueva via SSH mientras el contenedor viejo sigue sirviendo,
 * luego hace swap instantaneo con docker compose up -d.
 *
 * Diseñado para ser agnostico: funciona con cualquier stack Rust (o futuro stack)
 * configurado en settings.json con template="rust".
 *
 * Flujo:
 * 1. Sync compose con Coolify API (render template → PATCH)
 * 2. Verificar dependencias (postgres)
 * 3. Build imagen nueva (--no-cache para invalidar git clone)
 * 4. Swap contenedor (up -d --no-build)
 * 5. Conectar red traefik si es necesario
 * 6. Health check
 * 7. (Opcional) Ejecutar seed
 */

use crate::config::Settings;
use crate::domain::BackupTier;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::{backup_manager, health_manager, site_capabilities, volume_manager};
use std::path::Path;

mod compose_backup;
mod compose_sync;
mod compose_validation;
mod container_verification;
mod contexto;
mod env_building;
mod fases_inicio;
mod fases_nucleo;
mod host_preflight;
mod postgres_auth;
mod postgres_inspect;
mod rollback;
pub(crate) mod rust_autoheal;

use compose_backup::read_latest_compose_backup;
use compose_sync::{inject_traefik_network_label, sync_compose};
use container_verification::{
    verify_container_env_vars, verify_container_volumes, verify_postgres_data_volume,
};
use env_building::{build_env_from_coolify, runtime_envs_from_coolify};
use fases_inicio::{fase_preparar_host, fase_seguridad_backup, fase_sync_compose};
use fases_nucleo::{
    fase_build, fase_salud, fase_salud_colateral, fase_seed, fase_swap, fase_traefik,
};
use host_preflight::{
    check_server_resources, dominio_salud_resuelve, ensure_app_coolify_network,
    ensure_traefik_connected, extraer_host_salud, verify_or_inject_traefik_network_label,
    verify_postgres, wait_for_health,
};
use postgres_auth::ensure_postgres_auth_and_hostname;
use rust_autoheal::{
    command_output_summary, ensure_compose_service_image_available, install_rust_public_autoheal,
};

use contexto::CtxDeploy;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    skip_build: bool,
    seed: bool,
    skip_compose_sync: bool,
    skip_backup: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    let stack_uuid = site.stack_uuid.as_deref().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{site_name}' sin stackUuid configurado"))
    })?;
    let target = settings.resolve_site_target(site)?;
    let service_dir = format!("/data/coolify/services/{stack_uuid}");
    let compose_service = site_capabilities::resolve(site).app_name_hint.to_owned();

    fase_seguridad_backup(
        &settings,
        config_path,
        site_name,
        site,
        &target,
        skip_backup,
    )
    .await?;
    fase_sync_compose(config_path, site, stack_uuid, &target, skip_compose_sync).await?;

    /* --- 2. SSH + verificar postgres --- */
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;
    let ctx = CtxDeploy {
        settings: &settings,
        site,
        site_name,
        config_path,
        target,
        service_dir,
        compose_service,
        stack_uuid,
        skip_build,
        skip_compose_sync,
        seed,
    };

    let runtime_envs = fase_preparar_host(&ctx, &ssh).await?;

    fase_build(&ctx, &mut ssh, &runtime_envs).await?;

    fase_swap(&ctx, &ssh).await?;

    fase_traefik(&ctx, &ssh).await?;

    fase_salud(&ctx, &ssh).await?;

    fase_salud_colateral(&ctx, &ssh).await?;

    fase_seed(&ctx, &ssh).await?;

    Ok(())
}

#[cfg(test)]
mod network_recovery_tests {
    use super::env_building::{normalize_health_path, should_skip_runtime_compose_env};
    use super::rust_autoheal::{
        is_rust_network_probe_failure, shell_single_quote, systemd_safe_name,
    };
    use super::*;

    #[test]
    fn detects_rust_network_probe_failure_detail() {
        let report = health_manager::HealthReport {
            site_name: "studio".to_string(),
            url: "https://nakomi.studio/api/health".to_string(),
            http_ok: true,
            app_ok: false,
            fatal_log_detected: false,
            status_code: Some(200),
            details: vec!["Rust network probe fallo: exit=1".to_string()],
        };

        assert!(is_rust_network_probe_failure(&report));
    }

    #[test]
    fn shell_single_quote_escapes_recovery_values() {
        assert_eq!(shell_single_quote("studio'app"), "'studio'\\''app'");
    }

    #[test]
    fn systemd_safe_name_strips_unsafe_characters() {
        assert_eq!(systemd_safe_name("studio.prod"), "studio-prod");
        assert_eq!(systemd_safe_name("***"), "site");
    }

    #[test]
    fn command_output_summary_keeps_stdout_when_stderr_empty() {
        assert_eq!(
            command_output_summary("No such image: app\n", ""),
            "No such image: app"
        );
    }

    #[test]
    fn command_output_summary_combines_streams() {
        assert_eq!(
            command_output_summary("created", "warning"),
            "stdout:\ncreated\nstderr:\nwarning"
        );
    }

    #[test]
    fn normalize_health_path_keeps_compose_probe_valid() {
        assert_eq!(normalize_health_path("api/health"), "/api/health");
        assert_eq!(normalize_health_path("/swagger-ui/"), "/swagger-ui/");
        assert_eq!(normalize_health_path(""), "/");
    }

    #[test]
    fn runtime_compose_env_allows_prefixed_coolify_targets() {
        assert!(!should_skip_runtime_compose_env("COOLIFY_VPS1_BASE_URL"));
        assert!(!should_skip_runtime_compose_env("COOLIFY_VPS2_SERVER_UUID"));
        assert!(!should_skip_runtime_compose_env("COOLIFY_VPS10_API_TOKEN"));
    }

    #[test]
    fn cleanup_exited_cmd_filters_by_stack_uuid() {
        /* Regresión del incidente 2026-08-27: la limpieza de contenedores exited
         * debe limitarse al stack objetivo (label coolify.stack-uuid), nunca a
         * todos los contenedores del host (borró 9 sitios ajenos). */
        let stack_uuid = "do8k4w8swccwwogoc0os0ck0";
        let cmd = format!(
            "docker ps -a --filter label=coolify.stack-uuid={stack_uuid} --filter status=exited --format '{{{{.Names}}}}' | head -20"
        );
        assert!(cmd.contains("--filter label=coolify.stack-uuid=do8k4w8swccwwogoc0os0ck0"));
        assert!(cmd.contains("--filter status=exited"));
        assert!(!cmd.contains("docker ps -a --filter status=exited"));
    }

    #[test]
    fn runtime_compose_env_keeps_skipping_plain_coolify_platform_keys() {
        assert!(should_skip_runtime_compose_env("COOLIFY_BASE_URL"));
        assert!(should_skip_runtime_compose_env("COOLIFY_API_TOKEN"));
        assert!(should_skip_runtime_compose_env(
            "COOLIFY_VPS_ALPHA_BASE_URL"
        ));
        assert!(should_skip_runtime_compose_env("COOLIFY_VPS1_UNKNOWN"));
    }
}
