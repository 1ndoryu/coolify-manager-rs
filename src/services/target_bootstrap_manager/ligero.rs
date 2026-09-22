/* Split 119A-5 de target_bootstrap_manager.rs — bootstrap del runtime ligero.
 * Código verbatim del original; solo cambia visibilidad de ayudantes compartidos. */

use super::sondas::{command_exists, command_exists_any, path_exists, service_active};
use super::tipos::{BootstrapLightTargetReport, LightRuntimeState};
use crate::config::DeploymentTargetConfig;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

pub async fn bootstrap_target_light(
    target: &DeploymentTargetConfig,
    dry_run: bool,
) -> std::result::Result<BootstrapLightTargetReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let os_name = ssh
        .execute("bash -lc 'source /etc/os-release 2>/dev/null && echo ${PRETTY_NAME:-unknown}'")
        .await?
        .stdout
        .trim()
        .to_string();
    let before = inspect_light_runtime_state(&ssh).await?;
    let mut notes = vec![format!(
        "SO detectado: {}",
        if os_name.is_empty() {
            "unknown"
        } else {
            &os_name
        }
    )];
    notes.extend(summarize_light_runtime_state("detectado", &before));

    if dry_run {
        notes.push(
            "Dry-run: se asegurarian paquetes base (Docker, Caddy, MariaDB, Redis, UFW, fail2ban)."
                .to_string(),
        );
        notes.push(
            "Dry-run: se crearian /srv/hosting, /srv/backups/hosting y /etc/caddy/sites-enabled."
                .to_string(),
        );
        notes.push(
            "Dry-run: se habilitarian docker, caddy, mariadb, redis-server y fail2ban.".to_string(),
        );
        return Ok(BootstrapLightTargetReport {
            target: target.name.clone(),
            dry_run,
            services_ready: light_runtime_ready(&before),
            notes,
        });
    }

    let install_packages = ssh
        .execute("bash -lc 'export DEBIAN_FRONTEND=noninteractive; apt-get update -qq && (apt-get install -y -qq ca-certificates curl jq docker.io docker-compose-plugin caddy mariadb-server redis-server ufw fail2ban || apt-get install -y -qq ca-certificates curl jq docker.io docker-compose caddy mariadb-server redis-server ufw fail2ban)'")
        .await?;
    if !install_packages.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo instalando paquetes base del runtime ligero en '{}': {}",
            target.name, install_packages.stderr
        )));
    }
    notes.push(
        "Paquetes base instalados o verificados: Docker, Caddy, MariaDB, Redis, UFW y fail2ban."
            .to_string(),
    );

    let prepare_layout = ssh
        .execute("bash -lc 'mkdir -p /srv/hosting /srv/backups/hosting /etc/caddy/sites-available /etc/caddy/sites-enabled && chown root:root /srv/hosting /srv/backups /srv/backups/hosting /etc/caddy/sites-available /etc/caddy/sites-enabled && chmod 755 /srv/hosting /srv/backups /srv/backups/hosting /etc/caddy/sites-available /etc/caddy/sites-enabled'")
        .await?;
    if !prepare_layout.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo preparando layout del runtime ligero en '{}': {}",
            target.name, prepare_layout.stderr
        )));
    }
    notes.push("Layout base preparado en /srv/hosting y /srv/backups/hosting.".to_string());

    let caddy_baseline = ssh
        .execute("bash -lc \"if ! grep -q 'sites-enabled/\\*.caddy' /etc/caddy/Caddyfile 2>/dev/null; then printf '%s\\n' '{' '    email admin@localhost' '}' '' 'import /etc/caddy/sites-enabled/*.caddy' > /etc/caddy/Caddyfile; fi\"")
        .await?;
    if !caddy_baseline.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo configurando baseline de Caddy en '{}': {}",
            target.name, caddy_baseline.stderr
        )));
    }
    notes.push(
        "Baseline de Caddy asegurado con import de /etc/caddy/sites-enabled/*.caddy.".to_string(),
    );

    let enable_services = ssh
        .execute("bash -lc 'systemctl enable --now docker >/dev/null 2>&1 && systemctl enable --now caddy >/dev/null 2>&1 && systemctl enable --now mariadb >/dev/null 2>&1 && systemctl enable --now redis-server >/dev/null 2>&1 && systemctl enable --now fail2ban >/dev/null 2>&1'")
        .await?;
    if !enable_services.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo habilitando servicios del runtime ligero en '{}': {}",
            target.name, enable_services.stderr
        )));
    }
    notes.push(
        "Servicios base habilitados: docker, caddy, mariadb, redis-server y fail2ban.".to_string(),
    );
    notes.push(
        "Siguiente paso recomendado: optimize-host y enforce-host-security sobre el mismo target."
            .to_string(),
    );

    let after = inspect_light_runtime_state(&ssh).await?;
    notes.extend(summarize_light_runtime_state("listo", &after));

    Ok(BootstrapLightTargetReport {
        target: target.name.clone(),
        dry_run,
        services_ready: light_runtime_ready(&after),
        notes,
    })
}

pub(super) async fn inspect_light_runtime_state(
    ssh: &SshClient,
) -> std::result::Result<LightRuntimeState, CoolifyError> {
    Ok(LightRuntimeState {
        docker_installed: command_exists(ssh, "docker").await?,
        docker_active: service_active(ssh, "docker").await?,
        caddy_installed: command_exists(ssh, "caddy").await?,
        caddy_active: service_active(ssh, "caddy").await?,
        mariadb_installed: command_exists_any(ssh, &["mariadb", "mysql"]).await?,
        mariadb_active: service_active(ssh, "mariadb").await?,
        redis_installed: command_exists_any(ssh, &["redis-server", "redis-cli"]).await?,
        redis_active: service_active(ssh, "redis-server").await?,
        hosting_root_exists: path_exists(ssh, "/srv/hosting").await?,
        backups_root_exists: path_exists(ssh, "/srv/backups/hosting").await?,
        caddy_sites_dir_exists: path_exists(ssh, "/etc/caddy/sites-enabled").await?,
    })
}

pub(super) fn summarize_light_runtime_state(
    prefix: &str,
    state: &LightRuntimeState,
) -> Vec<String> {
    vec![
        format!(
            "Docker {prefix}: instalado={}, activo={}",
            state.docker_installed, state.docker_active
        ),
        format!(
            "Caddy {prefix}: instalado={}, activo={}",
            state.caddy_installed, state.caddy_active
        ),
        format!(
            "MariaDB {prefix}: instalado={}, activo={}",
            state.mariadb_installed, state.mariadb_active
        ),
        format!(
            "Redis {prefix}: instalado={}, activo={}",
            state.redis_installed, state.redis_active
        ),
        format!(
            "Ruta /srv/hosting {prefix}: {}",
            if state.hosting_root_exists {
                "presente"
            } else {
                "ausente"
            }
        ),
        format!(
            "Ruta /srv/backups/hosting {prefix}: {}",
            if state.backups_root_exists {
                "presente"
            } else {
                "ausente"
            }
        ),
        format!(
            "Ruta /etc/caddy/sites-enabled {prefix}: {}",
            if state.caddy_sites_dir_exists {
                "presente"
            } else {
                "ausente"
            }
        ),
    ]
}

pub(super) fn light_runtime_ready(state: &LightRuntimeState) -> bool {
    state.docker_installed
        && state.docker_active
        && state.caddy_installed
        && state.caddy_active
        && state.mariadb_installed
        && state.mariadb_active
        && state.redis_installed
        && state.redis_active
        && state.hosting_root_exists
        && state.backups_root_exists
        && state.caddy_sites_dir_exists
}
