/*
 * [276A-1] Comando: diagnose
 * Diagnostico completo de un sitio via SSH: contenedores, discos, BD, bind mounts, logs.
 * NO modifica nada — solo recolecta y reporta.
 *
 * Uso: coolify-manager diagnose --name kamples
 *      coolify-manager diagnose --name kamples --json
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use std::path::Path;

mod secciones;
use secciones::*;

/* [119A-3] Contexto compartido por todas las secciones del diagnóstico.
 * Evita pasar 7+ parámetros a cada fase (clippy::too-many-arguments). */
struct CtxDiagnostico<'a> {
    ssh: &'a SshClient,
    site_name: &'a str,
    stack_uuid: &'a str,
    template: String,
    target_name: String,
    api_token: String,
    base_url: String,
}

/* [119A-3] Informe recolectado por las secciones, listo para imprimir. */
struct InformeDiagnostico {
    containers: String,
    docker_df: String,
    volumes: String,
    pg_info: String,
    mariadb_info: String,
    compose: String,
    bind_mounts: String,
    logs: String,
    container_sizes: String,
    coolify_status: String,
    uploads_deep: String,
    pg_deep: String,
    mariadb_deep: String,
    wp_deep: String,
    long_uploads: String,
    pg_probe: String,
}

/* [119A-3] Inspección profunda de volúmenes (sección 11). */
struct Profundos {
    uploads: String,
    pg: String,
    mariadb: String,
    wp: String,
    largo: String,
}

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    json_output: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;

    let stack_uuid = match &site.stack_uuid {
        Some(u) => u.clone(),
        None => {
            return Err(CoolifyError::Validation(format!(
                "El sitio '{site_name}' no tiene stackUuid configurado"
            )));
        }
    };

    let target = settings.resolve_site_target(site)?;
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let ctx = CtxDiagnostico {
        ssh: &ssh,
        site_name,
        stack_uuid: &stack_uuid,
        template: site.template.to_string(),
        target_name: target.name.to_string(),
        api_token: settings.coolify.api_token.to_string(),
        base_url: settings.coolify.base_url.trim_end_matches('/').to_string(),
    };

    /* ── Recolectar secciones (orden secuencial SSH, pero agrupado) ── */

    let containers = seccion_contenedores(&ctx).await;

    let docker_df = seccion_docker_df(&ctx).await;

    let volumes = seccion_volumenes(&ctx).await;

    let pg_info = seccion_postgres(&ctx).await;

    let mariadb_info = seccion_mariadb(&ctx).await;

    let compose = seccion_compose(&ctx).await;

    let bind_mounts = seccion_bind_mounts(&ctx).await;

    let logs = seccion_logs(&ctx).await;

    let container_sizes = seccion_tamanos(&ctx).await;

    let coolify_status = seccion_coolify(&ctx).await;

    /* ── 11. Inspección profunda de volúmenes ── */
    let profundos = seccion_profundos(&ctx).await;

    /* ── 12. Sonda PostgreSQL (contenedor temporal, read-only) ── */
    let pg_probe = seccion_sonda_pg(&ctx).await;

    /* ── Ensamblar y mostrar el reporte ── */
    let informe = InformeDiagnostico {
        containers,
        docker_df,
        volumes,
        pg_info,
        mariadb_info,
        compose,
        bind_mounts,
        logs,
        container_sizes,
        coolify_status,
        uploads_deep: profundos.uploads,
        pg_deep: profundos.pg,
        mariadb_deep: profundos.mariadb,
        wp_deep: profundos.wp,
        long_uploads: profundos.largo,
        pg_probe,
    };
    mostrar_reporte(&ctx, &informe, json_output);

    Ok(())
}

/// Ejecuta un comando SSH y devuelve stdout + stderr.
/// Si falla, devuelve el fallback string proporcionado.
/* ── Ensamblar y mostrar el reporte ── */
fn mostrar_reporte(ctx: &CtxDiagnostico<'_>, inf: &InformeDiagnostico, json_output: bool) {
    if json_output {
        let report = serde_json::json!({
            "site": ctx.site_name,
            "stack_uuid": ctx.stack_uuid,
            "template": &ctx.template,
            "containers": inf.containers.trim(),
            "docker_df": inf.docker_df.trim(),
            "volumes": inf.volumes.trim(),
            "postgresql": inf.pg_info.trim(),
            "mariadb": inf.mariadb_info.trim(),
            "compose": inf.compose.trim(),
            "bind_mounts": inf.bind_mounts.trim(),
            "logs": inf.logs.trim(),
            "container_sizes": inf.container_sizes.trim(),
            "coolify_status": inf.coolify_status.trim(),
            "deep_uploads": inf.uploads_deep.trim(),
            "deep_postgres": inf.pg_deep.trim(),
            "deep_mariadb": inf.mariadb_deep.trim(),
            "deep_wordpress": inf.wp_deep.trim(),
            "deep_long_uploads": inf.long_uploads.trim(),
            "pg_probe": inf.pg_probe.trim(),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    } else {
        let header = format!(
            "═══ Diagnóstico: {} ═══\nUUID: {}\nTemplate: {}\nTarget: {}\n",
            ctx.site_name, ctx.stack_uuid, ctx.template, ctx.target_name,
        );
        println!("{header}");
        println!("── Contenedores del stack ──\n{}", inf.containers);
        println!("\n── Docker system df ──\n{}", inf.docker_df);
        println!("\n── Volúmenes Docker ──\n{}", inf.volumes);
        println!("\n── Base de datos PostgreSQL ──\n{}", inf.pg_info);
        println!("\n── Base de datos MySQL/MariaDB ──\n{}", inf.mariadb_info);
        println!(
            "\n── Docker Compose on-disk (primeras 80 líneas) ──\n{}",
            inf.compose
        );
        println!("\n── Bind mounts ──\n{}", inf.bind_mounts);
        println!(
            "\n── Logs del contenedor principal (últimas 30) ──\n{}",
            inf.logs
        );
        println!("\n── Tamaños de capas Docker ──\n{}", inf.container_sizes);
        println!("\n── Estado Coolify API ──\n{}", inf.coolify_status);
        println!(
            "\n── Inspección profunda: uploads-data ──\n{}",
            inf.uploads_deep
        );
        println!("\n── Inspección profunda: PostgreSQL ──\n{}", inf.pg_deep);
        println!("\n── Inspección profunda: MariaDB ──\n{}", inf.mariadb_deep);
        println!("\n── Inspección profunda: WordPress ──\n{}", inf.wp_deep);
        println!("\n── Volumen largo uploads-data ──\n{}", inf.long_uploads);
        println!(
            "\n── Sonda PostgreSQL (contenedor temporal) ──\n{}",
            inf.pg_probe
        );
        println!("\n═══ Fin del diagnóstico ── {} ═══", ctx.site_name);
    }
}
