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
use crate::domain::{BackupTier, BuildMode, SiteConfig};
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
pub(crate) mod env_building;
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
    image: Option<&str>,
) -> std::result::Result<(), CoolifyError> {
    let mut settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    /* [309A-1/F2] --image overridea imageRef de settings solo en memoria
     * (no escribe settings.json): permite desplegar un tag cm-local recién
     * subido sin editar configuración. */
    let site_owned: Option<SiteConfig>;
    let site: &SiteConfig = match image {
        Some(image_ref) => {
            validation::validate_image_ref(image_ref)?;
            site_owned = Some(SiteConfig {
                image_ref: Some(image_ref.to_string()),
                ..site.clone()
            });
            site_owned
                .as_ref()
                .expect("override de imagen recién creado")
        }
        None => site,
    };

    let stack_uuid = site.stack_uuid.as_deref().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{site_name}' sin stackUuid configurado"))
    })?;

    /* [01AA-3/F2] Modo laptop (sin --image y sin skip_build): compila en el
     * laptop y despliega con esa imagen (override en memoria, igual que
     * --image). --image explícito manda; skip_build conserva su semántica
     * (usa lo ya cargado, sin compilar). Templates sin build local: aviso
     * y ruta clásica (rama inerte, nunca rompe). */
    let site_owned_laptop: Option<SiteConfig>;
    let (site, tag_laptop): (&SiteConfig, Option<String>) =
        if image.is_none() && !skip_build && site.build_mode == BuildMode::Laptop {
            if BuildMode::tiene_build_local(&site.template) {
                println!("[2/6] build_mode=laptop: compilando en el laptop (sin build en VPS)...");
                let tag =
                    match super::build_laptop::execute(&super::build_laptop::ParamsBuildLaptop {
                        config_path,
                        site_name,
                        tag: None,
                        keep_tarball: false,
                        docker_bin: "docker",
                    })
                    .await
                    {
                        Ok(tag) => tag,
                        /* [01AA-3/F2c] El build falló: poda best-effort de colgadas
                         * (nunca tags) antes de devolver el error original. */
                        Err(e) => {
                            super::build_laptop::podar_colgadas("docker").await;
                            return Err(e);
                        }
                    };
                site_owned_laptop = Some(SiteConfig {
                    image_ref: Some(tag.clone()),
                    ..site.clone()
                });
                (
                    site_owned_laptop
                        .as_ref()
                        .expect("override laptop recién creado"),
                    Some(tag),
                )
            } else {
                println!(
                    "[2/6] build_mode=laptop pero template {:?} sin build local: ruta clásica VPS.",
                    site.template
                );
                (site, None)
            }
        } else {
            (site, None)
        };
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

    /* [01AA-3/F2x] Kamples con imagen (recién compilada en laptop o --image).
     * Lección E2E 2026-10-01 (triple copia de Coolify): el servicio guarda
     * `image` (atributo, solo-lectura por API) + `docker_compose_raw` +
     * `docker_compose` materializado. El PATCH del raw persiste, pero un
     * reconciliador de Coolify regenera raw+disco desde el atributo `image`
     * (viejo) en minutos: editar el yml on-disk por SSH y `up -d` manual
     * funciona unos minutos y luego revierte (status=exited). El camino
     * durable es el endpoint oficial `POST /api/v1/deploy`: materializa el
     * raw, recrea contenedores y neutraliza el atributo (queda vacío).
     * Por eso este tail NO para/arranca ni toca el disco: sync DB (fase
     * anterior) → deploy oficial → espera con veredicto → health →
     * fix-db-auth. Las fases build/swap/salud/seed/colateral de abajo son
     * específicas de Rust. Kamples sin imagen (modo vps) sigue cayendo al
     * error claro de sync_compose. */
    if matches!(site.template, crate::domain::StackTemplate::Kamples) && site.image_ref.is_some() {
        println!("[3/6] Kamples con imagen: deploy oficial de Coolify (materializa el compose)...");
        let image_ref = site.image_ref.as_deref().unwrap_or_default();
        let api = CoolifyApiClient::new(&target.coolify)?;
        api.deploy_stack(stack_uuid).await?;
        let mut ssh_up = SshClient::from_vps(&target.vps);
        ssh_up.connect().await?;
        let mut desplegado = false;
        /* E2E 2026-10-01: el deploy oficial tarda ~3 min en recrear
         * wordpress (el pipeline recrea también mariadb/postgres). Con 4
         * intentos el poll expiraba justo antes de converger aunque todo
         * terminaba bien; margen a 8×30 s (4 min). */
        const INTENTOS_DEPLOY: usize = 8;
        for intento in 1..=INTENTOS_DEPLOY {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let ps = ssh_up
                .execute(&format!(
                    "docker ps --format '{{{{.Names}}}} {{{{.Image}}}} {{{{.Status}}}}' | grep -i '{stack_uuid}'"
                ))
                .await?;
            /* El poll exige estado "Up": el pipeline recrea también
             * mariadb/postgres y wordpress puede pasar por "Restarting"
             * (crash-loop hasta que mariadb está healthy) durante minutos.
             * La salud real la valida el health gate posterior; aquí solo
             * se verifica materialización de la imagen en ejecución. */
            if ps.stdout.contains(image_ref) && ps.stdout.contains(" Up ") {
                println!(
                    "      contenedores con la imagen nueva: OK (intento {intento}/{INTENTOS_DEPLOY})."
                );
                desplegado = true;
                break;
            }
            println!("      esperando materialización... (intento {intento}/{INTENTOS_DEPLOY})");
        }
        if !desplegado {
            return Err(CoolifyError::Validation(format!(
                "Coolify no levantó '{site_name}' con la imagen '{image_ref}' tras el deploy oficial"
            )));
        }
        /* [01AA-3/F2x] Rotación ANTES del health gate: la limpieza es
         * best-effort e independiente del veredicto de salud; si el health
         * falla (p. ej. sitio sin DNS aún), el tag recién subido quedaría
         * como basural. El tag en despliegue es siempre el más nuevo y la
         * rotación conserva los 2 últimos, así que nunca borra lo que corre.
         * Con --image no hay build y `tag_laptop` es None: se rota con el
         * image_ref en despliegue (misma forma: ref completa). */
        let tag_rotacion: Option<&str> = tag_laptop.as_deref().or(site.image_ref.as_deref());
        if let Some(tag) = tag_rotacion {
            super::build_laptop::rotar_etiquetas_laptop("docker", site_name, tag).await;
            super::build_laptop::rotar_etiquetas_vps(&mut ssh_up, site_name, tag).await;
        }
        let mut saludable = false;
        for intento in 1..=3 {
            match health_manager::assert_site_healthy(&settings, site, &ssh_up).await {
                Ok(report) if report.healthy() => {
                    println!("Health check: OK — deploy Kamples exitoso.");
                    saludable = true;
                    break;
                }
                Ok(report) => {
                    for detail in &report.details {
                        println!("  - {detail}");
                    }
                }
                Err(e) => println!("  - health intento {intento}/3: {e}"),
            }
            if intento < 3 {
                tokio::time::sleep(std::time::Duration::from_secs(20)).await;
            }
        }
        if !saludable {
            return Err(CoolifyError::Validation(format!(
                "Deploy Kamples completado pero '{site_name}' no pasó health check tras 3 intentos"
            )));
        }
        println!("Verificando sincronización de credenciales DB post-deploy...");
        match super::fix_db_auth::execute(config_path, site_name, false).await {
            Ok(_) => println!("Credenciales DB: OK"),
            Err(e) => {
                eprintln!("WARN: fix-db-auth reportó: {e}");
                eprintln!("      Si el sitio está caído, ejecuta: fix-db-auth --name {site_name}");
            }
        }
        /* [01AA-3/F3] Deploy verde: el tag en ejecución es la verdad y se
         * persiste (solo rama laptop; con --image settings ya lo trae). */
        if let Some(tag) = tag_laptop.as_deref() {
            if let Err(e) = persistir_image_ref(&mut settings, config_path, site_name, tag) {
                eprintln!("WARN: deploy OK pero no se persistió imageRef: {e}");
            }
        }
        println!("deploy-service Kamples completado para '{site_name}'.");
        return Ok(());
    }

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

    /* [01AA-3/F2b] Rotación automática (best-effort, no aborta el deploy
     * ya verde): conserva las 2 últimas cm-local/<sitio> en laptop y VPS. */
    if let Some(tag) = tag_laptop.as_deref() {
        super::build_laptop::rotar_etiquetas_laptop("docker", site_name, tag).await;
        super::build_laptop::rotar_etiquetas_vps(&mut ssh, site_name, tag).await;
        /* [01AA-3/F3] Igual que la rama Kamples: persiste el tag en ejecución. */
        if let Err(e) = persistir_image_ref(&mut settings, config_path, site_name, tag) {
            eprintln!("WARN: deploy OK pero no se persistió imageRef: {e}");
        }
    }

    Ok(())
}

/* [01AA-3/F3] Tras un deploy laptop verde, el tag recién subido es la verdad
 * en ejecución y se persiste en settings: sin esto, un futuro rollback a
 * `vps` (o un restart/health) usaría el image_ref viejo, ya podado por la
 * rotación N=2, y fallaría fail-closed. Solo la rama laptop escribe (con
 * --image settings ya trae el valor). Best-effort en el llamante: si el
 * guardado falla se avisa con WARN sin invalidar el deploy. */
fn persistir_image_ref(
    settings: &mut Settings,
    config_path: &Path,
    site_name: &str,
    tag: &str,
) -> std::result::Result<(), CoolifyError> {
    match settings.sitios.iter_mut().find(|s| s.nombre == site_name) {
        Some(sitio) => {
            sitio.image_ref = Some(tag.to_string());
            settings.save(config_path)?;
            println!("settings: imageRef actualizado a '{tag}'.");
            Ok(())
        }
        None => Err(CoolifyError::Validation(format!(
            "Sitio '{site_name}' no encontrado en settings para persistir imageRef"
        ))),
    }
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

    /* [01AA-3/F3] El tag desplegado por la rama laptop queda persistido en
     * settings (roundtrip a disco); en sitio inexistente falla sin escribir. */
    #[test]
    fn persiste_image_ref_tras_deploy_laptop() {
        use std::io::Write as _;
        let json = r#"{
            "vps": {"ip": "1.2.3.4", "user": "root"},
            "coolify": {
                "baseUrl": "http://1.2.3.4:8000",
                "apiToken": "test-token",
                "serverUuid": "srv-1",
                "projectUuid": "proj-1"
            },
            "wordpress": {
                "dbUser": "manager",
                "dbPassword": "secret",
                "defaultAdminEmail": "a@b.com"
            },
            "glory": {
                "templateRepo": "https://github.com/test/template.git",
                "libraryRepo": "https://github.com/test/lib.git"
            },
            "sitios": [
                {"nombre": "demo", "dominio": "https://demo.test", "buildMode": "laptop"}
            ]
        }"#;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(json.as_bytes()).unwrap();
        f.flush().unwrap();

        let tag = "cm-local/demo:20261005-120000";
        let mut settings = Settings::load(f.path()).unwrap();
        assert!(settings.get_site("demo").unwrap().image_ref.is_none());
        persistir_image_ref(&mut settings, f.path(), "demo", tag).unwrap();

        let recargado = Settings::load(f.path()).unwrap();
        let sitio = recargado.get_site("demo").unwrap();
        assert_eq!(sitio.image_ref.as_deref(), Some(tag));
        assert_eq!(
            sitio.build_mode,
            crate::domain::BuildMode::Laptop,
            "persistir el tag no debe tocar build_mode"
        );

        let mut settings = Settings::load(f.path()).unwrap();
        assert!(persistir_image_ref(&mut settings, f.path(), "fantasma", tag).is_err());
    }
}
