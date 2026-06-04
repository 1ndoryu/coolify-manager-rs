use crate::config::DeploymentTargetConfig;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PurgeDockerHostReport {
    pub target: String,
    pub dry_run: bool,
    pub all_data: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
struct DockerHostState {
    containers: Vec<String>,
    volumes: Vec<String>,
    custom_networks: Vec<String>,
    image_count: usize,
}

pub async fn purge_target(
    target: &DeploymentTargetConfig,
    all_data: bool,
    dry_run: bool,
) -> std::result::Result<PurgeDockerHostReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let before = inspect_host_state(&ssh).await?;
    let mut notes = summarize_state("detectados", &before);

    if dry_run {
        notes.push("Dry-run: no se removio ningun recurso Docker remoto.".to_string());
        if all_data {
            notes.push("Dry-run: se purgarian volumenes, redes custom, imagenes no usadas y builder cache.".to_string());
        }
        return Ok(PurgeDockerHostReport {
            target: target.name.clone(),
            dry_run,
            all_data,
            notes,
        });
    }

    notes.extend(remove_containers(&ssh, &before.containers).await?);
    if all_data {
        notes.extend(
            remove_persistent_docker_data(&mut ssh, &before.volumes, &before.custom_networks)
                .await?,
        );
    } else {
        notes.push(
            "Volumenes, redes custom e imagenes conservados (usa --all-data para purgarlos)."
                .to_string(),
        );
    }

    let after = inspect_host_state(&ssh).await?;
    notes.extend(summarize_state("restantes", &after));

    Ok(PurgeDockerHostReport {
        target: target.name.clone(),
        dry_run,
        all_data,
        notes,
    })
}

async fn inspect_host_state(ssh: &SshClient) -> std::result::Result<DockerHostState, CoolifyError> {
    Ok(DockerHostState {
        containers: list_items(ssh, "docker ps -a --format '{{.Names}}' 2>/dev/null || true").await?,
        volumes: list_items(ssh, "docker volume ls --format '{{.Name}}' 2>/dev/null || true").await?,
        custom_networks: list_items(
            ssh,
            "docker network ls --format '{{.Name}}' 2>/dev/null | grep -Ev '^(bridge|host|none)$' || true",
        )
        .await?,
        image_count: count_items(ssh, "docker image ls -q 2>/dev/null | sort -u || true").await?,
    })
}

async fn remove_containers(
    ssh: &SshClient,
    containers: &[String],
) -> std::result::Result<Vec<String>, CoolifyError> {
    if containers.is_empty() {
        return Ok(vec![
            "No habia contenedores Docker para remover.".to_string()
        ]);
    }

    let command = format!(
        "docker rm -f {} >/dev/null 2>&1 || true",
        containers.join(" ")
    );
    ssh.execute(&format!("bash -lc {}", sh_quote(&command)))
        .await?;

    Ok(vec![format!(
        "Contenedores removidos: {}",
        containers.join(",")
    )])
}

async fn remove_persistent_docker_data(
    ssh: &mut SshClient,
    volumes: &[String],
    custom_networks: &[String],
) -> std::result::Result<Vec<String>, CoolifyError> {
    let mut notes = Vec::new();

    if volumes.is_empty() {
        notes.push("No habia volumenes Docker para remover.".to_string());
    } else {
        let command = format!(
            "docker volume rm -f {} >/dev/null 2>&1 || true",
            volumes.join(" ")
        );
        ssh.execute(&format!("bash -lc {}", sh_quote(&command)))
            .await?;
        notes.push(format!("Volumenes removidos: {}", volumes.join(",")));
    }

    if custom_networks.is_empty() {
        notes.push("No habia redes Docker custom para remover.".to_string());
    } else {
        let command = format!(
            "for network in {}; do docker network rm \"$network\" >/dev/null 2>&1 || true; done",
            custom_networks.join(" ")
        );
        ssh.execute(&format!("bash -lc {}", sh_quote(&command)))
            .await?;
        notes.push(format!(
            "Redes custom intentadas para remocion: {}",
            custom_networks.join(",")
        ));
    }

    ssh.execute_long_running(
        "docker image prune -af",
        "/tmp/coolify-manager-image-prune.log",
        2,
        1800,
    )
    .await?;
    ssh.execute_long_running(
        "docker builder prune -af",
        "/tmp/coolify-manager-builder-prune.log",
        2,
        1800,
    )
    .await?;
    notes.push("Imagenes no usadas y builder cache purgados.".to_string());

    Ok(notes)
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

async fn count_items(ssh: &SshClient, command: &str) -> std::result::Result<usize, CoolifyError> {
    let output = ssh.execute(command).await?;
    Ok(output
        .stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count())
}

fn summarize_state(prefix: &str, state: &DockerHostState) -> Vec<String> {
    vec![
        format!(
            "Contenedores {prefix}: {}",
            describe_items(&state.containers)
        ),
        format!("Volumenes {prefix}: {}", describe_items(&state.volumes)),
        format!(
            "Redes custom {prefix}: {}",
            describe_items(&state.custom_networks)
        ),
        format!("Imagenes {prefix}: {}", state.image_count),
    ]
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
