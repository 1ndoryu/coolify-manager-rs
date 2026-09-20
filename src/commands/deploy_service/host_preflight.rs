use super::rust_autoheal::{
    command_output_summary, is_rust_network_probe_failure, recover_rust_network_probe_failure,
    shell_single_quote,
};
use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::services::health_manager;
use std::net::ToSocketAddrs;

/* [119A-2/B0] E11: extraer el host de una URL de salud (con o sin esquema,
 * con o sin path/puerto) para poder verificar resolución DNS. */
pub(crate) fn extraer_host_salud(url: &str) -> &str {
    let sin_esquema = url.split_once("://").map(|(_, resto)| resto).unwrap_or(url);
    let sin_path = sin_esquema.split('/').next().unwrap_or(sin_esquema);
    sin_path.split('@').next_back().unwrap_or(sin_path)
}

/* [119A-2/B0] E11: true si el dominio de la URL de salud resuelve por DNS.
 * Un sitio NUEVO sin DNS propagado falla el health HTTPS aunque el contenedor
 * esté healthy; en ese caso el rollback es ciego (bucle rebuild ~10 min/ciclo)
 * y debe omitirse con warning en vez de tratarse como "app rota". */
pub(crate) fn dominio_salud_resuelve(url: &str) -> bool {
    let host = extraer_host_salud(url);
    if host.is_empty() {
        return false;
    }
    let puerto = if url.trim_start().starts_with("http://") {
        80
    } else {
        443
    };
    format!("{host}:{puerto}")
        .to_socket_addrs()
        .map(|mut addrs| addrs.next().is_some())
        .unwrap_or(false)
}

/* [214A-4] Verificar que el servidor tenga suficiente RAM y disco antes del build.
 * Un build Docker puede necesitar ~1GB+ de RAM y varios GB de disco para layers.
 * Si falla a mitad, deja basura en disco que empeora la situación.
 * Umbrales conservadores: ≥512MB RAM disponible, ≥3GB disco libre.
 * Se puede anular con la variable de entorno SKIP_RESOURCE_CHECK=1. */
pub(crate) async fn check_server_resources(
    ssh: &SshClient,
    service_dir: &str,
) -> std::result::Result<(), CoolifyError> {
    const MIN_RAM_MB: u64 = 512;
    const MIN_DISK_GB: u64 = 3;

    /* RAM: columna "available" de free -m (incluye buffers/cache reutilizable) */
    let mem_result = ssh.execute("free -m | awk '/^Mem:/ {print $7}'").await?;
    let available_mb: u64 = mem_result.stdout.trim().parse().unwrap_or(0);

    if available_mb > 0 && available_mb < MIN_RAM_MB {
        return Err(CoolifyError::Validation(format!(
            "RAM insuficiente: {available_mb}MB disponibles (mínimo {MIN_RAM_MB}MB). \
             Libera memoria antes de hacer build. SKIP_RESOURCE_CHECK=1 para forzar."
        )));
    }

    /* Disco: espacio libre en la partición donde vive el servicio */
    let disk_result = ssh
        .execute(&format!(
            "df {} 2>/dev/null | awk 'NR==2 {{print $4}}'",
            service_dir
        ))
        .await?;
    let free_kb: u64 = disk_result.stdout.trim().parse().unwrap_or(0);
    let free_gb = free_kb / 1_048_576;

    if free_kb > 0 && free_gb < MIN_DISK_GB {
        return Err(CoolifyError::Validation(format!(
            "Disco insuficiente: {free_gb}GB libres en {service_dir} (mínimo {MIN_DISK_GB}GB). \
             Limpia imágenes Docker: docker system prune -af. SKIP_RESOURCE_CHECK=1 para forzar."
        )));
    }

    println!(
        "      Recursos OK: {}MB RAM, {}GB disco libres",
        available_mb, free_gb
    );
    Ok(())
}

/* Verifica que postgres este corriendo. Si no, lo inicia y espera a que este healthy. */
pub(crate) async fn verify_postgres(
    ssh: &SshClient,
    service_dir: &str,
) -> std::result::Result<(), CoolifyError> {
    let status_cmd = format!(
        "cd {} && docker compose ps postgres --format '{{{{.Status}}}}' 2>/dev/null",
        service_dir
    );
    let status = ssh.execute(&status_cmd).await?;
    let status_text = status.stdout.trim();

    if status_text.contains("Up")
        || status_text.contains("running")
        || status_text.contains("healthy")
    {
        return Ok(());
    }

    tracing::info!("Postgres no esta corriendo, iniciando...");
    let start_cmd = format!("cd {} && docker compose up -d postgres 2>&1", service_dir);
    ssh.execute(&start_cmd).await?;

    /* Esperar hasta 60s a que postgres este healthy */
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let check = ssh.execute(&status_cmd).await?;
        if check.stdout.contains("healthy") {
            return Ok(());
        }
    }

    Err(CoolifyError::Validation(
        "Postgres no alcanzo estado healthy en 60s".to_string(),
    ))
}

/* [incident-2026-07-21] Verificar que traefik.docker.network=coolify existe en compose on-disk.
 * Si Coolify regeneró el compose durante el build y el label se perdió, inyectarlo via sed.
 * Sin este label, Traefik no puede encontrar el contenedor → 503 "no available server". */
pub(crate) async fn verify_or_inject_traefik_network_label(
    ssh: &SshClient,
    service_dir: &str,
) -> std::result::Result<(), CoolifyError> {
    let check_cmd = format!(
        "grep -q 'traefik.docker.network=coolify' {}/docker-compose.yml && echo OK || echo MISSING",
        service_dir
    );
    let check = ssh.execute(&check_cmd).await?;
    if check.stdout.trim() == "OK" {
        return Ok(());
    }
    /* Label faltante — inyectar después de traefik.enable=true */
    tracing::warn!(
        "traefik.docker.network=coolify no encontrado en compose on-disk, inyectando..."
    );
    let inject_cmd = format!(
        "sed -i '/traefik.enable=true/a\\      - traefik.docker.network=coolify' {}/docker-compose.yml",
        service_dir
    );
    ssh.execute(&inject_cmd).await?;
    /* Verificar que se inyectó */
    let verify = ssh.execute(&check_cmd).await?;
    if verify.stdout.trim() != "OK" {
        return Err(CoolifyError::Validation(
            "No se pudo inyectar traefik.docker.network=coolify en compose on-disk".to_string(),
        ));
    }
    eprintln!("      Label traefik.docker.network=coolify inyectado via sed.");
    Ok(())
}

/* Asegura que el proxy Traefik de Coolify pueda alcanzar la red del servicio */
pub(crate) async fn ensure_traefik_connected(
    ssh: &SshClient,
    service_network: &str,
) -> std::result::Result<(), CoolifyError> {
    let inspect_cmd = format!(
        "docker network inspect {} --format '{{{{range .Containers}}}}{{{{.Name}}}} {{{{end}}}}' 2>/dev/null",
        service_network
    );
    let result = ssh.execute(&inspect_cmd).await?;

    if !result.stdout.contains("coolify-proxy") {
        tracing::info!("Conectando Traefik a la red del servicio...");
        let connect_cmd = format!(
            "docker network connect {} coolify-proxy 2>/dev/null || true",
            service_network
        );
        ssh.execute(&connect_cmd).await?;
    }
    Ok(())
}

pub(crate) async fn ensure_app_coolify_network(
    ssh: &SshClient,
    service_dir: &str,
    compose_service: &str,
) -> std::result::Result<(), CoolifyError> {
    let service_dir_quoted = shell_single_quote(service_dir);
    let compose_service_quoted = shell_single_quote(compose_service);
    let command = format!(
        "cd {service_dir_quoted} || exit 2; \
         cid=$(docker compose ps -q {compose_service_quoted} 2>/dev/null || true); \
         if [ -z \"$cid\" ]; then echo 'WARN: app container missing for coolify network'; exit 0; fi; \
         if ! docker network inspect coolify >/dev/null 2>&1; then echo 'WARN: coolify network missing'; exit 0; fi; \
         docker network connect coolify \"$cid\" 2>/dev/null || true; \
         if docker exec \"$cid\" getent hosts coolify >/dev/null 2>&1; then echo 'coolify network ready'; else echo 'WARN: coolify hostname unresolved from app'; fi"
    );
    let result = ssh.execute(&command).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo conectar app a la red interna de Coolify: {}",
            command_output_summary(&result.stdout, &result.stderr)
        )));
    }
    if !result.stdout.trim().is_empty() {
        println!("      {}", result.stdout.trim().replace('\n', "\n      "));
    }
    Ok(())
}

/* Espera hasta 120s a que el health check pase */
pub(crate) async fn wait_for_health(
    settings: &Settings,
    site: &crate::domain::SiteConfig,
    ssh: &SshClient,
    service_dir: &str,
    compose_service: &str,
) -> std::result::Result<health_manager::HealthReport, CoolifyError> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let mut network_recreate_attempted = false;

    while std::time::Instant::now() < deadline {
        match health_manager::run_site_health_check(settings, site, ssh).await {
            Ok(report) if report.healthy() => return Ok(report),
            Ok(report) => {
                if !network_recreate_attempted && is_rust_network_probe_failure(&report) {
                    network_recreate_attempted = true;
                    tracing::warn!(
                        "Rust network probe fallo; recreando {compose_service} sin build una vez"
                    );
                    recover_rust_network_probe_failure(ssh, service_dir, compose_service, site)
                        .await?;
                    tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                    continue;
                }
                let remaining = (deadline - std::time::Instant::now()).as_secs();
                tracing::debug!("Health check no paso aun, {remaining}s restantes");
            }
            Err(e) => {
                tracing::debug!("Health check error: {e}, reintentando...");
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }

    /* Ultimo intento: si falla, retornar el error */
    health_manager::assert_site_healthy(settings, site, ssh).await
}

#[cfg(test)]
mod tests {
    use super::{dominio_salud_resuelve, extraer_host_salud};

    #[test]
    fn b0_extrae_host_de_url_salud() {
        assert_eq!(
            extraer_host_salud("https://cm-test-119a2.wandori.us/api/health"),
            "cm-test-119a2.wandori.us"
        );
        assert_eq!(extraer_host_salud("https://example.com"), "example.com");
        assert_eq!(
            extraer_host_salud("http://example.com:8080/x"),
            "example.com:8080"
        );
        assert_eq!(
            extraer_host_salud("cm-test-119a2.wandori.us/api/health"),
            "cm-test-119a2.wandori.us"
        );
        assert_eq!(extraer_host_salud(""), "");
    }

    #[test]
    fn b0_localhost_siempre_resuelve() {
        assert!(dominio_salud_resuelve("http://localhost/api/health"));
    }
}
