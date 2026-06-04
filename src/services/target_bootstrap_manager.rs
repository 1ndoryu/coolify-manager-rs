use crate::config::DeploymentTargetConfig;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct InstallCoolifyReport {
    pub target: String,
    pub access_url: String,
    pub os_name: String,
    pub already_installed: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BootstrapLightTargetReport {
    pub target: String,
    pub dry_run: bool,
    pub services_ready: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UninstallCoolifyReport {
    pub target: String,
    pub dry_run: bool,
    pub purge_data: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
struct CoolifyResourceState {
    containers: Vec<String>,
    volumes: Vec<String>,
    networks: Vec<String>,
    data_path_exists: bool,
}

#[derive(Debug, Clone)]
struct LightRuntimeState {
    docker_installed: bool,
    docker_active: bool,
    caddy_installed: bool,
    caddy_active: bool,
    mariadb_installed: bool,
    mariadb_active: bool,
    redis_installed: bool,
    redis_active: bool,
    hosting_root_exists: bool,
    backups_root_exists: bool,
    caddy_sites_dir_exists: bool,
}

pub async fn install_coolify(
    target: &DeploymentTargetConfig,
) -> std::result::Result<InstallCoolifyReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let os_name = ssh
        .execute("bash -lc 'source /etc/os-release 2>/dev/null && echo ${PRETTY_NAME:-unknown}'")
        .await?
        .stdout
        .trim()
        .to_string();
    let mut notes = vec![format!(
        "SO detectado: {}",
        if os_name.is_empty() {
            "unknown"
        } else {
            &os_name
        }
    )];

    let before = ssh
        .execute("bash -lc \"docker ps --format '{{.Names}}' 2>/dev/null | grep -E '^coolify($|-)|^coolify-db$|^coolify-redis$' || true\"")
        .await?;
    let already_installed = !before.stdout.trim().is_empty();

    if already_installed {
        notes.push("Coolify ya parece instalado; se omite reinstalacion.".to_string());
    } else {
        notes.push("Ejecutando instalador oficial de Coolify.".to_string());
        let install_result = ssh
            .execute("bash -lc 'export DEBIAN_FRONTEND=noninteractive; apt-get update -qq >/dev/null 2>&1 || true; apt-get install -y -qq curl >/dev/null 2>&1 || true; curl -fsSL https://cdn.coollabs.io/coolify/install.sh | bash'")
            .await?;
        if !install_result.success() {
            return Err(CoolifyError::Validation(format!(
                "Fallo la instalacion de Coolify en '{}': {}",
                target.name, install_result.stderr
            )));
        }
        notes.push("Instalador oficial completado sin error de shell.".to_string());
    }

    let docker_check = ssh
        .execute("bash -lc \"docker ps --format '{{.Names}}' 2>/dev/null | grep -E '^coolify($|-)|^coolify-db$|^coolify-redis$' || true\"")
        .await?;
    if docker_check.stdout.trim().is_empty() {
        return Err(CoolifyError::Validation(format!(
            "No se detectaron contenedores de Coolify en '{}' despues de la instalacion",
            target.name
        )));
    }

    let http_status = ssh
        .execute(
            "bash -lc \"curl -fsS -o /dev/null -w '%{http_code}' http://127.0.0.1:8000 || true\"",
        )
        .await?
        .stdout
        .trim()
        .to_string();
    if http_status == "200" || http_status == "302" {
        notes.push(format!(
            "Coolify responde localmente con HTTP {}.",
            http_status
        ));
    } else {
        notes.push(format!(
            "Coolify aun no responde con 200/302 en localhost:8000 (actual: {}).",
            if http_status.is_empty() {
                "sin respuesta"
            } else {
                &http_status
            }
        ));
    }

    notes.push(
        "Despues de instalar hay que abrir la UI y crear el primer admin manualmente.".to_string(),
    );
    notes.push("El apiToken y los UUID de server/project solo existirán despues de completar el registro inicial en Coolify.".to_string());

    Ok(InstallCoolifyReport {
        target: target.name.clone(),
        access_url: format!("http://{}:8000", target.vps.ip),
        os_name,
        already_installed,
        notes,
    })
}

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

pub async fn uninstall_coolify(
    target: &DeploymentTargetConfig,
    purge_data: bool,
    dry_run: bool,
) -> std::result::Result<UninstallCoolifyReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let before = inspect_coolify_resources(&ssh).await?;
    let mut notes = summarize_resource_state("detectados", &before);

    if dry_run {
        notes.push("Dry-run: no se removio ningun recurso remoto.".to_string());
        if purge_data {
            notes.push("Dry-run: se purgaria /data/coolify y los volumenes coolify*.".to_string());
        }
        return Ok(UninstallCoolifyReport {
            target: target.name.clone(),
            dry_run,
            purge_data,
            notes,
        });
    }

    notes.extend(remove_coolify_resources(&ssh, &before, purge_data).await?);

    let after = inspect_coolify_resources(&ssh).await?;
    notes.extend(summarize_resource_state("restantes", &after));

    Ok(UninstallCoolifyReport {
        target: target.name.clone(),
        dry_run,
        purge_data,
        notes,
    })
}

async fn inspect_coolify_resources(
    ssh: &SshClient,
) -> std::result::Result<CoolifyResourceState, CoolifyError> {
    Ok(CoolifyResourceState {
        containers: list_items(
            ssh,
            "docker ps -a --format '{{.Names}}' 2>/dev/null | grep '^coolify' || true",
        )
        .await?,
        volumes: list_items(
            ssh,
            "docker volume ls --format '{{.Name}}' 2>/dev/null | grep '^coolify' || true",
        )
        .await?,
        networks: list_items(
            ssh,
            "docker network ls --format '{{.Name}}' 2>/dev/null | grep '^coolify' || true",
        )
        .await?,
        data_path_exists: path_exists(ssh, "/data/coolify").await?,
    })
}

async fn inspect_light_runtime_state(
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

async fn remove_coolify_resources(
    ssh: &SshClient,
    state: &CoolifyResourceState,
    purge_data: bool,
) -> std::result::Result<Vec<String>, CoolifyError> {
    let mut notes = Vec::new();

    if !state.containers.is_empty() {
        let remove_containers = format!(
            "docker rm -f {} >/dev/null 2>&1 || true",
            state.containers.join(" ")
        );
        ssh.execute(&format!("bash -lc {}", sh_quote(&remove_containers)))
            .await?;
        notes.push(format!(
            "Contenedores removidos: {}",
            state.containers.join(",")
        ));
    } else {
        notes.push("No habia contenedores coolify* para remover.".to_string());
    }

    if purge_data {
        notes.extend(remove_coolify_persistent_state(ssh, state).await?);
    } else {
        notes.push("Volumenes conservados (usa --purge-data para eliminarlos).".to_string());
        notes.push("Ruta /data/coolify conservada (usa --purge-data para eliminarla).".to_string());
    }

    if !state.networks.is_empty() {
        let remove_networks = format!(
            "for network in {}; do docker network rm \"$network\" >/dev/null 2>&1 || true; done",
            state.networks.join(" ")
        );
        ssh.execute(&format!("bash -lc {}", sh_quote(&remove_networks)))
            .await?;
        notes.push(format!(
            "Redes intentadas para remocion: {}",
            state.networks.join(",")
        ));
    } else {
        notes.push("No habia redes coolify* para remover.".to_string());
    }

    Ok(notes)
}

async fn remove_coolify_persistent_state(
    ssh: &SshClient,
    state: &CoolifyResourceState,
) -> std::result::Result<Vec<String>, CoolifyError> {
    let mut notes = Vec::new();

    if !state.volumes.is_empty() {
        let remove_volumes = format!(
            "docker volume rm -f {} >/dev/null 2>&1 || true",
            state.volumes.join(" ")
        );
        ssh.execute(&format!("bash -lc {}", sh_quote(&remove_volumes)))
            .await?;
        notes.push(format!("Volumenes removidos: {}", state.volumes.join(",")));
    } else {
        notes.push("No habia volumenes coolify* para remover.".to_string());
    }

    if state.data_path_exists {
        ssh.execute("bash -lc 'rm -rf /data/coolify'").await?;
        notes.push("Ruta /data/coolify removida.".to_string());
    } else {
        notes.push("Ruta /data/coolify ya estaba ausente.".to_string());
    }

    Ok(notes)
}

fn summarize_resource_state(prefix: &str, state: &CoolifyResourceState) -> Vec<String> {
    vec![
        format!(
            "Contenedores {prefix}: {}",
            describe_items(&state.containers)
        ),
        format!("Volumenes {prefix}: {}", describe_items(&state.volumes)),
        format!("Redes {prefix}: {}", describe_items(&state.networks)),
        format!(
            "Ruta /data/coolify {prefix}: {}",
            if state.data_path_exists {
                "presente"
            } else {
                "ausente"
            }
        ),
    ]
}

fn summarize_light_runtime_state(prefix: &str, state: &LightRuntimeState) -> Vec<String> {
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

fn light_runtime_ready(state: &LightRuntimeState) -> bool {
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

async fn list_items(
    ssh: &SshClient,
    command: &str,
) -> std::result::Result<Vec<String>, CoolifyError> {
    let output = ssh.execute(command).await?;
    Ok(output
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect())
}

async fn command_exists(
    ssh: &SshClient,
    command_name: &str,
) -> std::result::Result<bool, CoolifyError> {
    let result = ssh
        .execute(&format!(
            "bash -lc 'command -v {} >/dev/null 2>&1 && echo yes || echo no'",
            command_name
        ))
        .await?;
    Ok(result.stdout.trim() == "yes")
}

async fn command_exists_any(
    ssh: &SshClient,
    command_names: &[&str],
) -> std::result::Result<bool, CoolifyError> {
    for command_name in command_names {
        if command_exists(ssh, command_name).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn service_active(
    ssh: &SshClient,
    service_name: &str,
) -> std::result::Result<bool, CoolifyError> {
    let result = ssh
        .execute(&format!(
            "bash -lc 'systemctl is-active {} >/dev/null 2>&1 && echo yes || echo no'",
            service_name
        ))
        .await?;
    Ok(result.stdout.trim() == "yes")
}

async fn path_exists(ssh: &SshClient, path: &str) -> std::result::Result<bool, CoolifyError> {
    let result = ssh
        .execute(&format!(
            "bash -lc 'test -e {} && echo yes || echo no'",
            shell_escape(path)
        ))
        .await?;
    Ok(result.stdout.trim() == "yes")
}

fn describe_items(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(",")
    }
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn shell_escape(value: &str) -> String {
    sh_quote(value)
}
