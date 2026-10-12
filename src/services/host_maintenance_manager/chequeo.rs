use crate::config::DeploymentTargetConfig;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use serde::Serialize;

use super::{detect_installed_kernel, empty_as_unknown, exec_trim, reboot_required, sh_quote};

/// Solo lectura sobre paquetes y servicios. Lo unico que escribe es el indice de apt (`apt-get update`).
const SCRIPT_CHEQUEO: &str = r#"export LC_ALL=C DEBIAN_FRONTEND=noninteractive
if apt-get update -qq >/dev/null 2>&1; then echo apt_index=ok; else echo apt_index=fallo; fi
echo "upgradable=$(apt list --upgradable 2>/dev/null | sed '1d' | wc -l)"
echo "held=$(apt-mark showhold 2>/dev/null | tr '\n' ' ')"
echo "failed=$(systemctl --failed --no-legend --plain 2>/dev/null | awk '{print $1}' | tr '\n' ' ')"
"#;

#[derive(Debug, Clone, Serialize)]
pub struct HostCheckReport {
    pub target: String,
    pub os_name: String,
    pub running_kernel: String,
    pub installed_kernel: String,
    pub reboot_required: bool,
    pub apt_index_updated: bool,
    pub upgradable_packages: Option<u64>,
    pub held_packages: Vec<String>,
    pub failed_units: Vec<String>,
}

pub async fn check_target(
    target: &DeploymentTargetConfig,
) -> std::result::Result<HostCheckReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let os_name = exec_trim(
        &ssh,
        "sh -lc '. /etc/os-release 2>/dev/null; echo ${PRETTY_NAME:-unknown}'",
    )
    .await?;
    let running_kernel = exec_trim(&ssh, "uname -r").await?;
    let installed_kernel = detect_installed_kernel(&ssh).await?;
    let pendiente_reinicio = reboot_required(&ssh, &running_kernel, &installed_kernel).await?;

    // Mismo patron que run_package_maintenance: apt puede tardar mas que un ssh.execute corto.
    let command = format!("bash -lc {}", sh_quote(SCRIPT_CHEQUEO));
    let safe_target = target
        .name
        .replace(|c: char| !c.is_ascii_alphanumeric(), "_");
    let log_file = format!("/tmp/cm-check-host-{}.log", safe_target);
    let resultado = ssh
        .execute_long_running(&command, &log_file, 10, 900)
        .await?;
    if !resultado.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo consultando el estado del host: {}{}",
            resultado.stdout, resultado.stderr
        )));
    }
    let chequeo = parsear_chequeo(&resultado.stdout);

    Ok(HostCheckReport {
        target: target.name.clone(),
        os_name: empty_as_unknown(&os_name),
        running_kernel: empty_as_unknown(&running_kernel),
        installed_kernel: empty_as_unknown(&installed_kernel),
        reboot_required: pendiente_reinicio,
        apt_index_updated: chequeo.apt_index_updated,
        upgradable_packages: chequeo.upgradable,
        held_packages: chequeo.held,
        failed_units: chequeo.failed,
    })
}

#[derive(Debug, Default, PartialEq)]
struct ChequeoRemoto {
    apt_index_updated: bool,
    upgradable: Option<u64>,
    held: Vec<String>,
    failed: Vec<String>,
}

fn parsear_chequeo(salida: &str) -> ChequeoRemoto {
    let mut chequeo = ChequeoRemoto::default();
    for linea in salida.lines().map(str::trim) {
        if let Some(valor) = linea.strip_prefix("apt_index=") {
            chequeo.apt_index_updated = valor == "ok";
        } else if let Some(valor) = linea.strip_prefix("upgradable=") {
            chequeo.upgradable = valor.parse().ok();
        } else if let Some(valor) = linea.strip_prefix("held=") {
            chequeo.held = valor.split_whitespace().map(str::to_string).collect();
        } else if let Some(valor) = linea.strip_prefix("failed=") {
            chequeo.failed = valor.split_whitespace().map(str::to_string).collect();
        }
    }
    chequeo
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsear_chequeo_lee_todos_los_campos() {
        let chequeo = parsear_chequeo(
            "apt_index=ok\nupgradable=12\nheld=linux-image-x docker-ce \nfailed=foo.service \n",
        );
        assert!(chequeo.apt_index_updated);
        assert_eq!(chequeo.upgradable, Some(12));
        assert_eq!(chequeo.held, ["linux-image-x", "docker-ce"]);
        assert_eq!(chequeo.failed, ["foo.service"]);
    }

    #[test]
    fn parsear_chequeo_sin_datos_queda_en_desconocido() {
        let chequeo = parsear_chequeo("apt_index=fallo\nupgradable=\nheld=\nfailed=\n");
        assert!(!chequeo.apt_index_updated);
        assert_eq!(chequeo.upgradable, None);
        assert!(chequeo.held.is_empty());
        assert!(chequeo.failed.is_empty());
    }
}
