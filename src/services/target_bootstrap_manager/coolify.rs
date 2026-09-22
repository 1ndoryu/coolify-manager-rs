/* Split 119A-5 de target_bootstrap_manager.rs — install/uninstall de Coolify.
 * Código verbatim del original; solo cambia visibilidad de ayudantes compartidos. */

use super::sondas::{describe_items, list_items, path_exists, sh_quote};
use super::tipos::{CoolifyResourceState, InstallCoolifyReport, UninstallCoolifyReport};
use crate::config::DeploymentTargetConfig;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

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

pub(super) async fn inspect_coolify_resources(
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

pub(super) async fn remove_coolify_resources(
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

pub(super) async fn remove_coolify_persistent_state(
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

pub(super) fn summarize_resource_state(prefix: &str, state: &CoolifyResourceState) -> Vec<String> {
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
