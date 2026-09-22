/* Split 119A-5 de lightweight_runtime_manager.rs — inventario, alta y reporte.
 * Codigo verbatim del original; solo cambia visibilidad de ayudantes compartidos. */

use super::plantillas::{
    build_caddyfile, build_default_index, build_provision_static_script, build_site_metadata,
    build_sshd_match, build_static_compose, generate_access_password, generate_access_user,
    sh_quote,
};
use super::tipos::{
    LightweightInventoryReport, LightweightSiteAction, LightweightSiteActionReport,
    LightweightSiteInventory, ProvisionStaticSiteReport,
};
use crate::config::DeploymentTargetConfig;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

pub(super) const LIGHTWEIGHT_INVENTORY_SCRIPT: &str = r#"
set -euo pipefail
shopt -s nullglob
for dir in /srv/hosting/*; do
    [ -d "$dir" ] || continue
    site=$(basename "$dir")
    compose=""
    for candidate in "$dir/docker-compose.yml" "$dir/docker-compose.yaml" "$dir/compose.yml" "$dir/compose.yaml"; do
        if [ -f "$candidate" ]; then
            compose="$candidate"
            break
        fi
    done

    all_containers=$(docker ps -a --filter "label=com.docker.compose.project=$site" --format "{{.Names}}" 2>/dev/null || true)
    running_containers=$(docker ps --filter "label=com.docker.compose.project=$site" --format "{{.Names}}" 2>/dev/null || true)
    total=$(printf "%s\n" "$all_containers" | sed '/^$/d' | wc -l | tr -d ' ')
    running=$(printf "%s\n" "$running_containers" | sed '/^$/d' | wc -l | tr -d ' ')
    containers=$(printf "%s\n" "$all_containers" | sed '/^$/d' | paste -sd ',' -)

    if [ -z "$compose" ]; then
        status="incomplete"
    elif [ "$total" = "0" ]; then
        status="defined"
    elif [ "$running" = "$total" ]; then
        status="running"
    elif [ "$running" = "0" ]; then
        status="stopped"
    else
        status="degraded"
    fi

    caddy_file="/etc/caddy/sites-enabled/$site.caddy"
    fqdn=""
    if [ -f "$caddy_file" ]; then
        fqdn=$(awk 'NF && $1 !~ /^#/ && $1 != "{" {print $1; exit}' "$caddy_file" || true)
    fi

    public_root=""
    if [ -d "$dir/public" ]; then
        public_root="$dir/public"
    fi

    printf "%s\t%s\t%s\t%s\t%s\t%s\n" "$site" "$status" "$fqdn" "$dir" "$public_root" "$containers"
done
"#;

pub async fn inventory_light_target(
    target: &DeploymentTargetConfig,
) -> std::result::Result<LightweightInventoryReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let sites = list_lightweight_sites(&ssh).await?;
    Ok(LightweightInventoryReport {
        target: target.name.clone(),
        target_ip: target.vps.ip.clone(),
        sites,
    })
}

/* [245A-9] provision_static_site ya era el entrypoint central del alta lightweight.
 * Este bloque cierra backup/restore alrededor sin mezclar un refactor mayor. */
// sentinel-disable-next-line limite-lineas
pub async fn provision_static_site(
    target: &DeploymentTargetConfig,
    site_name: &str,
    fqdn: Option<&str>,
    access_user: Option<&str>,
    access_password: Option<&str>,
) -> std::result::Result<ProvisionStaticSiteReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let site_name = site_name.trim();
    if site_name.is_empty() {
        return Err(CoolifyError::Validation(
            "El deployment lightweight requiere un nombre de sitio no vacío".to_string(),
        ));
    }

    let access_user = access_user
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| generate_access_user(site_name));
    let access_password = access_password
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(generate_access_password);
    let fqdn = fqdn
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{site_name}.{}.sslip.io", target.vps.ip));
    let http_port = allocate_http_port(&ssh).await?;

    let project_root = format!("/srv/hosting/{site_name}");
    let public_root = format!("{project_root}/public");
    let compose_file = format!("{project_root}/compose.yml");
    let metadata_file = format!("{project_root}/site.env");
    let caddy_available = format!("/etc/caddy/sites-available/{site_name}.caddy");
    let caddy_enabled = format!("/etc/caddy/sites-enabled/{site_name}.caddy");
    let sshd_match_file = format!("/etc/ssh/sshd_config.d/hosting-{site_name}.conf");

    let compose_yaml = build_static_compose(http_port);
    let caddyfile = build_caddyfile(&fqdn, http_port);
    let sshd_match = build_sshd_match(site_name, &access_user);
    let metadata = build_site_metadata(&access_user, http_port, &fqdn);
    let index_html = build_default_index(site_name, &fqdn);
    let script = build_provision_static_script(
        &project_root,
        &public_root,
        &compose_file,
        &metadata_file,
        &caddy_available,
        &caddy_enabled,
        &sshd_match_file,
        &access_user,
        &access_password,
        &compose_yaml,
        &metadata,
        &caddyfile,
        &sshd_match,
        &index_html,
        http_port,
    );

    let output = ssh
        .execute(&format!("bash -lc {}", sh_quote(&script)))
        .await?;
    if !output.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo provisionando sitio lightweight '{}': {}",
            site_name, output.stderr
        )));
    }

    Ok(ProvisionStaticSiteReport {
        target: target.name.clone(),
        target_ip: target.vps.ip.clone(),
        deployment_id: site_name.to_string(),
        fqdn: fqdn.clone(),
        public_url: format!("https://{fqdn}"),
        project_root,
        public_root,
        access_user,
        access_password,
        access_port: 22,
    })
}

pub(super) fn action_report(
    target: &DeploymentTargetConfig,
    site: LightweightSiteInventory,
    action: LightweightSiteAction,
    notes: Vec<String>,
) -> LightweightSiteActionReport {
    LightweightSiteActionReport {
        target: target.name.clone(),
        target_ip: target.vps.ip.clone(),
        site: site.name,
        action: action.as_str().to_string(),
        status: site.status,
        fqdn: site.fqdn,
        notes,
    }
}

pub(super) async fn require_site(
    ssh: &SshClient,
    site_name: &str,
) -> std::result::Result<LightweightSiteInventory, CoolifyError> {
    list_lightweight_sites(ssh)
        .await?
        .into_iter()
        .find(|site| site.name == site_name || site.deployment_id == site_name)
        .ok_or_else(|| {
            CoolifyError::Validation(format!(
                "Sitio lightweight '{}' no encontrado en /srv/hosting",
                site_name
            ))
        })
}

/* [245A-9] list_lightweight_sites sigue siendo el parser central del runtime.
 * Dividirlo aqui mezclaria deuda estructural previa con el contrato de backups. */
// sentinel-disable-next-line limite-lineas
pub(super) async fn list_lightweight_sites(
    ssh: &SshClient,
) -> std::result::Result<Vec<LightweightSiteInventory>, CoolifyError> {
    let output = ssh
        .execute(&format!(
            "bash -lc {}",
            sh_quote(LIGHTWEIGHT_INVENTORY_SCRIPT)
        ))
        .await?;
    if !output.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo listando sitios lightweight: {}",
            output.stderr
        )));
    }

    parse_inventory_tsv(&output.stdout)
}

pub(super) fn parse_inventory_tsv(
    raw: &str,
) -> std::result::Result<Vec<LightweightSiteInventory>, CoolifyError> {
    let mut sites = Vec::new();

    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let mut parts = line.splitn(6, '\t');
        let name = parts
            .next()
            .ok_or_else(|| {
                CoolifyError::Validation(format!(
                    "Linea de inventario lightweight invalida (name faltante): {line}"
                ))
            })?
            .trim();
        let status = parts
            .next()
            .ok_or_else(|| {
                CoolifyError::Validation(format!(
                    "Linea de inventario lightweight invalida (status faltante): {line}"
                ))
            })?
            .trim();
        let fqdn = parts.next().unwrap_or_default().trim();
        let project_root = parts.next().unwrap_or_default().trim();
        let public_root = parts.next().unwrap_or_default().trim();
        let containers = parts.next().unwrap_or_default().trim();

        sites.push(LightweightSiteInventory {
            deployment_id: name.to_string(),
            name: name.to_string(),
            status: status.to_string(),
            fqdn: (!fqdn.is_empty()).then(|| fqdn.to_string()),
            project_root: project_root.to_string(),
            public_root: (!public_root.is_empty()).then(|| public_root.to_string()),
            containers: containers
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect(),
        });
    }

    Ok(sites)
}

pub(super) async fn allocate_http_port(ssh: &SshClient) -> std::result::Result<u16, CoolifyError> {
    let script = [
        "set -euo pipefail",
        "for candidate in $(seq 21000 23999); do",
        "  if ! ss -ltn | awk '{print $4}' | grep -E \"(^|:)${candidate}$\" >/dev/null 2>&1; then",
        "    printf '%s' \"$candidate\"",
        "    exit 0",
        "  fi",
        "done",
        "exit 21",
    ]
    .join("\n");
    let output = ssh
        .execute(&format!("bash -lc {}", sh_quote(&script)))
        .await?;
    if !output.success() {
        return Err(CoolifyError::Validation(
            "No se pudo reservar un puerto HTTP libre para el runtime lightweight".to_string(),
        ));
    }

    output.stdout.trim().parse::<u16>().map_err(|error| {
        CoolifyError::Validation(format!(
            "Puerto HTTP inválido devuelto por el runtime lightweight: {}",
            error
        ))
    })
}

pub(super) async fn resolve_compose_file(
    ssh: &SshClient,
    site_name: &str,
) -> std::result::Result<String, CoolifyError> {
    let script = [
        "set -euo pipefail".to_string(),
        format!("site_root=/srv/hosting/{}", site_name),
        "for candidate in \"$site_root/docker-compose.yml\" \"$site_root/docker-compose.yaml\" \"$site_root/compose.yml\" \"$site_root/compose.yaml\"; do".to_string(),
        "  if [ -f \"$candidate\" ]; then".to_string(),
        "    printf '%s' \"$candidate\"".to_string(),
        "    exit 0".to_string(),
        "  fi".to_string(),
        "done".to_string(),
        "exit 11".to_string(),
    ]
    .join("\n");
    let output = ssh
        .execute(&format!("bash -lc {}", sh_quote(&script)))
        .await?;

    if output.success() {
        return Ok(output.stdout.trim().to_string());
    }

    Err(CoolifyError::Validation(format!(
        "Sitio lightweight '{}' sin compose en /srv/hosting/{}/",
        site_name, site_name
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_inventory_tsv_splits_optional_fields() {
        let raw = "site-a\trunning\texample.com\t/srv/hosting/site-a\t/srv/hosting/site-a/public\tweb,php\nsite-b\tdefined\t\t/srv/hosting/site-b\t\t\n";
        let parsed = parse_inventory_tsv(raw).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "site-a");
        assert_eq!(parsed[0].containers, vec!["web", "php"]);
        assert_eq!(parsed[1].fqdn, None);
        assert!(parsed[1].containers.is_empty());
    }
}
