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
