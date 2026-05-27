/* sentinel-disable-file limite-lineas: servicio central del runtime lightweight.
 * El archivo ya concentraba inventario/provisioning/control antes de 245A-9; este
 * bloque cierra backup/restore sin mezclar un refactor estructural de 1000+ lineas. */
use crate::config::{DeploymentTargetConfig, Settings};
use crate::domain::BackupTier;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::services::backup_manager::{self, BackupArtifact, BackupManifest, BackupStatus};

use base64::Engine;
use chrono::Utc;
use flate2::write::GzEncoder;
use flate2::Compression;
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::fs::File;
use std::path::Path;
use tar::Builder;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LightweightSiteInventory {
    pub deployment_id: String,
    pub name: String,
    pub status: String,
    pub fqdn: Option<String>,
    pub project_root: String,
    pub public_root: Option<String>,
    pub containers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightweightInventoryReport {
    pub target: String,
    pub target_ip: String,
    pub sites: Vec<LightweightSiteInventory>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightweightSiteAction {
    Start,
    Stop,
    Restart,
    Reconfigure,
    Delete,
}

impl LightweightSiteAction {
    pub fn parse(raw: &str) -> std::result::Result<Self, CoolifyError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "start" => Ok(Self::Start),
            "stop" => Ok(Self::Stop),
            "restart" => Ok(Self::Restart),
            "reconfigure" => Ok(Self::Reconfigure),
            "delete" => Ok(Self::Delete),
            _ => Err(CoolifyError::Validation(format!(
                "Accion lightweight no soportada '{}'. Usa start, stop, restart, reconfigure o delete.",
                raw
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Reconfigure => "reconfigure",
            Self::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightweightSiteActionReport {
    pub target: String,
    pub target_ip: String,
    pub site: String,
    pub action: String,
    pub status: String,
    pub fqdn: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisionStaticSiteReport {
    pub target: String,
    pub target_ip: String,
    pub deployment_id: String,
    pub fqdn: String,
    pub public_url: String,
    pub project_root: String,
    pub public_root: String,
    pub access_user: String,
    pub access_password: String,
    pub access_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightweightBackupEntry {
    pub backup_id: String,
    pub tier: String,
    pub file_id: String,
    pub file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightweightBackupListReport {
    pub target: String,
    pub target_ip: String,
    pub site: String,
    pub entries: Vec<LightweightBackupEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightweightBackupReport {
    pub target: String,
    pub target_ip: String,
    pub site: String,
    pub backup_id: String,
    pub tier: String,
    pub status: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightweightRestoreReport {
    pub target: String,
    pub target_ip: String,
    pub site: String,
    pub backup_id: String,
    pub status: String,
    pub fqdn: Option<String>,
    pub access_user: Option<String>,
    pub access_password: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Default)]
struct RestoreOutputMetadata {
    fqdn: Option<String>,
    access_user: Option<String>,
    access_password: Option<String>,
}

const LIGHTWEIGHT_INVENTORY_SCRIPT: &str = r#"
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

pub async fn control_lightweight_site(
    target: &DeploymentTargetConfig,
    site_name: &str,
    action: LightweightSiteAction,
    fqdn: Option<&str>,
    access_user: Option<&str>,
    access_password: Option<&str>,
    delete_volumes: bool,
) -> std::result::Result<LightweightSiteActionReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    match action {
        LightweightSiteAction::Delete => {
            delete_site(&ssh, site_name, delete_volumes).await?;
            Ok(LightweightSiteActionReport {
                target: target.name.clone(),
                target_ip: target.vps.ip.clone(),
                site: site_name.to_string(),
                action: action.as_str().to_string(),
                status: "deleted".to_string(),
                fqdn: None,
                notes: vec![if delete_volumes {
                    "Sitio eliminado junto con su directorio de proyecto.".to_string()
                } else {
                    "Sitio desmontado y proyecto preservado bajo /srv/backups/hosting/deleted/."
                        .to_string()
                }],
            })
        }
        LightweightSiteAction::Reconfigure => {
            reconfigure_site(&ssh, site_name, fqdn, access_user, access_password).await?;
            let site = require_site(&ssh, site_name).await?;
            let mut notes = Vec::new();
            if fqdn.is_some() {
                notes.push("Dominio/Caddy actualizado para el sitio lightweight.".to_string());
            }
            if access_password.is_some() {
                notes.push("Password SFTP actualizada en el host compartido.".to_string());
            }
            Ok(action_report(target, site, action, notes))
        }
        _ => {
            run_compose_action(&ssh, site_name, action).await?;
            let site = require_site(&ssh, site_name).await?;
            let mut notes = Vec::new();
            if action == LightweightSiteAction::Restart {
                notes.push(
                    "Si no habia contenedores previos, el compose se levantó en modo up -d."
                        .to_string(),
                );
            }
            Ok(action_report(target, site, action, notes))
        }
    }
}

pub async fn list_lightweight_site_backups(
    settings: &Settings,
    config_path: &Path,
    target: &DeploymentTargetConfig,
    site_name: &str,
) -> std::result::Result<LightweightBackupListReport, CoolifyError> {
    let entries = backup_manager::list_site_backups(settings, config_path, site_name).await?;
    Ok(LightweightBackupListReport {
        target: target.name.clone(),
        target_ip: target.vps.ip.clone(),
        site: site_name.to_string(),
        entries: entries
            .into_iter()
            .map(|entry| LightweightBackupEntry {
                backup_id: entry.backup_id,
                tier: entry.tier.to_string(),
                file_id: entry.file_id,
                file_name: entry.file_name,
            })
            .collect(),
    })
}

/* [245A-9] El backup lightweight debe cerrar el ciclo completo del runtime.
 * No basta con empaquetar /srv/hosting/{site}: el restore tiene que rehacer
 * Caddy/SSH y devolver la password regenerada para resincronizar el panel. */
pub async fn create_lightweight_site_backup(
    settings: &Settings,
    config_path: &Path,
    target: &DeploymentTargetConfig,
    site_name: &str,
    tier: BackupTier,
    label: Option<&str>,
) -> std::result::Result<LightweightBackupReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let site = require_site(&ssh, site_name).await?;
    let backup_id = build_backup_id(label);
    let local_root = std::env::temp_dir().join(format!("cm-light-backup-{backup_id}"));
    let staging_dir = local_root.join(&backup_id);
    fs::create_dir_all(&staging_dir)?;

    let artifact_name = format!("files-{}.tar.gz", sanitize_path_name(&site.project_root));
    let local_artifact = staging_dir.join(&artifact_name);
    let local_archive = local_root.join(format!("{backup_id}.tar.gz"));
    let remote_archive = format!("/tmp/cm-lightweight-backup-{backup_id}.tar.gz");

    let archive_script = vec![
        "set -euo pipefail".to_string(),
        format!("site_root={}", sh_quote(&site.project_root)),
        format!("archive={}", sh_quote(&remote_archive)),
        "if [ ! -d \"$site_root\" ]; then echo \"Sitio lightweight inexistente\" >&2; exit 24; fi"
            .to_string(),
        "tar -czf \"$archive\" -C / \"${site_root#/}\"".to_string(),
    ]
    .join("\n");

    let archive_result = ssh
        .execute(&format!("bash -lc {}", sh_quote(&archive_script)))
        .await?;
    if !archive_result.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo creando backup lightweight para '{}': {}",
            site_name, archive_result.stderr
        )));
    }

    let download_result = ssh.download_file_streamed(&remote_archive, &local_artifact).await;
    let _ = ssh
        .execute(&format!("rm -f {}", sh_quote(&remote_archive)))
        .await;
    download_result?;

    let manifest = BackupManifest {
        backup_id: backup_id.clone(),
        site_name: site.name.clone(),
        tier: tier.clone(),
        status: BackupStatus::Ready,
        created_at: Utc::now(),
        label: label.map(str::to_string),
        artifacts: vec![build_local_artifact(
            "files",
            &sanitize_path_name(&site.project_root),
            &local_artifact,
            Some(site.project_root.clone()),
        )?],
        notes: vec![
            "runtime=lightweight".to_string(),
            format!("source={}", site.project_root),
        ],
    };

    write_local_manifest(&staging_dir, &manifest)?;
    create_local_archive(&staging_dir, &local_archive)?;

    let upload_result = backup_manager::upload_site_backup_archive(
        settings,
        config_path,
        &site.name,
        &tier,
        &backup_id,
        &local_archive,
    )
    .await;

    let _ = cleanup_dir(&local_root);

    let file_id = upload_result?;
    if let Some(keep) = retention_keep_for(&tier) {
        if let Err(error) = backup_manager::prune_site_backup_retention(
            settings,
            config_path,
            &site.name,
            &tier,
            keep,
        )
        .await
        {
            tracing::warn!(
                "No se pudo podar backups lightweight de '{}': {}",
                site_name,
                error
            );
        }
    }

    Ok(LightweightBackupReport {
        target: target.name.clone(),
        target_ip: target.vps.ip.clone(),
        site: site.name,
        backup_id,
        tier: tier.to_string(),
        status: "ready".to_string(),
        notes: vec![format!("remote.id={file_id}"), "runtime=lightweight".to_string()],
    })
}

pub async fn restore_lightweight_site_backup(
    settings: &Settings,
    config_path: &Path,
    target: &DeploymentTargetConfig,
    site_name: &str,
    backup_id: &str,
    access_password: Option<&str>,
    skip_safety_snapshot: bool,
) -> std::result::Result<LightweightRestoreReport, CoolifyError> {
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let existing_site = list_lightweight_sites(&ssh)
        .await?
        .into_iter()
        .find(|site| site.name == site_name || site.deployment_id == site_name);
    let safety_snapshot = if skip_safety_snapshot || existing_site.is_none() {
        None
    } else {
        Some(
            create_lightweight_site_backup(
                settings,
                config_path,
                target,
                site_name,
                BackupTier::Manual,
                Some("pre-restore"),
            )
            .await?
            .backup_id,
        )
    };

    let manifest_dir = backup_manager::materialize_site_backup(
        settings,
        config_path,
        site_name,
        backup_id,
    )
    .await?
    .ok_or_else(|| {
        CoolifyError::Validation(format!(
            "Backup '{}' no encontrado para '{}'",
            backup_id, site_name
        ))
    })?;
    let manifest = read_local_manifest(&manifest_dir.join("manifest.json"))?;
    validate_local_manifest(&manifest_dir, &manifest)?;

    let Some(files_artifact) = manifest.artifacts.iter().find(|artifact| artifact.kind == "files")
    else {
        let _ = cleanup_dir(manifest_dir.parent().unwrap_or(&manifest_dir));
        return Err(CoolifyError::Validation(format!(
            "Backup '{}' de '{}' no contiene artifacts de archivos",
            backup_id, site_name
        )));
    };

    let local_artifact = manifest_dir.join(&files_artifact.relative_path);
    let remote_artifact = format!("/tmp/cm-lightweight-restore-{backup_id}.tar.gz");
    ssh.upload_file_streamed(&local_artifact, &remote_artifact).await?;

    let restore_script = build_restore_script(site_name, &remote_artifact, access_password);
    let restore_result = ssh
        .execute(&format!("bash -lc {}", sh_quote(&restore_script)))
        .await;
    let _ = ssh
        .execute(&format!("rm -f {}", sh_quote(&remote_artifact)))
        .await;
    let _ = cleanup_dir(manifest_dir.parent().unwrap_or(&manifest_dir));

    let restore_result = restore_result?;
    if !restore_result.success() {
        if let Some(safety_backup_id) = safety_snapshot.as_deref() {
            tracing::error!(
                "Restore lightweight '{}' falló; intentando rollback con {}",
                site_name,
                safety_backup_id
            );
            let _ = Box::pin(restore_lightweight_site_backup(
                settings,
                config_path,
                target,
                site_name,
                safety_backup_id,
                access_password,
                true,
            ))
            .await;
        }
        return Err(CoolifyError::Validation(format!(
            "Fallo restaurando backup '{}' de '{}': {}",
            backup_id, site_name, restore_result.stderr
        )));
    }

    let output = parse_restore_output(&restore_result.stdout);
    let mut notes = vec!["runtime=lightweight".to_string()];
    if safety_snapshot.is_some() {
        notes.push("safety_snapshot=created".to_string());
    }
    if output.access_password.is_some() {
        notes.push("access_password=rotated".to_string());
    }

    Ok(LightweightRestoreReport {
        target: target.name.clone(),
        target_ip: target.vps.ip.clone(),
        site: site_name.to_string(),
        backup_id: backup_id.to_string(),
        status: "restored".to_string(),
        fqdn: output.fqdn,
        access_user: output.access_user,
        access_password: output.access_password,
        notes,
    })
}

fn action_report(
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

async fn require_site(
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
async fn list_lightweight_sites(
    ssh: &SshClient,
) -> std::result::Result<Vec<LightweightSiteInventory>, CoolifyError> {
    let output = ssh
                .execute(&format!("bash -lc {}", sh_quote(LIGHTWEIGHT_INVENTORY_SCRIPT)))
        .await?;
    if !output.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo listando sitios lightweight: {}",
            output.stderr
        )));
    }

    parse_inventory_tsv(&output.stdout)
}

fn parse_inventory_tsv(
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

async fn run_compose_action(
    ssh: &SshClient,
    site_name: &str,
    action: LightweightSiteAction,
) -> std::result::Result<(), CoolifyError> {
    let compose_file = resolve_compose_file(ssh, site_name).await?;
    let action_line = match action {
        LightweightSiteAction::Start => {
            "compose -p \"$site\" -f \"$compose_file\" up -d".to_string()
        }
        LightweightSiteAction::Stop => {
            "compose -p \"$site\" -f \"$compose_file\" stop".to_string()
        }
        LightweightSiteAction::Restart => "compose -p \"$site\" -f \"$compose_file\" restart >/dev/null 2>&1 || compose -p \"$site\" -f \"$compose_file\" up -d".to_string(),
        LightweightSiteAction::Reconfigure => unreachable!("reconfigure usa reconfigure_site"),
        LightweightSiteAction::Delete => unreachable!("delete usa delete_site"),
    };

    let script = vec![
        "set -euo pipefail".to_string(),
        "compose() {".to_string(),
        "  if docker compose version >/dev/null 2>&1; then".to_string(),
        "    docker compose \"$@\"".to_string(),
        "  elif command -v docker-compose >/dev/null 2>&1; then".to_string(),
        "    docker-compose \"$@\"".to_string(),
        "  else".to_string(),
        "    echo \"docker compose no esta disponible\" >&2".to_string(),
        "    exit 1".to_string(),
        "  fi".to_string(),
        "}".to_string(),
        format!("site={}", sh_quote(site_name)),
        format!("compose_file={}", sh_quote(&compose_file)),
        action_line,
    ]
    .join("\n");

    let output = ssh
        .execute(&format!("bash -lc {}", sh_quote(&script)))
        .await?;
    if !output.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo ejecutando '{}' sobre '{}': {}",
            action.as_str(),
            site_name,
            output.stderr
        )));
    }
    Ok(())
}

async fn delete_site(
    ssh: &SshClient,
    site_name: &str,
    delete_volumes: bool,
) -> std::result::Result<(), CoolifyError> {
    let compose_file = resolve_compose_file(ssh, site_name).await?;
    let delete_flag = if delete_volumes { "-v" } else { "" };
    let script = vec![
        "set -euo pipefail".to_string(),
        "compose() {".to_string(),
        "  if docker compose version >/dev/null 2>&1; then".to_string(),
        "    docker compose \"$@\"".to_string(),
        "  elif command -v docker-compose >/dev/null 2>&1; then".to_string(),
        "    docker-compose \"$@\"".to_string(),
        "  else".to_string(),
        "    echo \"docker compose no esta disponible\" >&2".to_string(),
        "    exit 1".to_string(),
        "  fi".to_string(),
        "}".to_string(),
        format!("site={}", sh_quote(site_name)),
        format!("compose_file={}", sh_quote(&compose_file)),
        "site_root=\"/srv/hosting/$site\"".to_string(),
        "deleted_root=\"/srv/backups/hosting/deleted\"".to_string(),
        "metadata_file=\"$site_root/site.env\"".to_string(),
        format!(
            "compose -p \"$site\" -f \"$compose_file\" down --remove-orphans {} >/dev/null 2>&1 || true",
            delete_flag
        ),
        "rm -f \"/etc/caddy/sites-enabled/$site.caddy\" \"/etc/caddy/sites-available/$site.caddy\"".to_string(),
        "if [ -f \"$metadata_file\" ]; then . \"$metadata_file\"; fi".to_string(),
        "if [ -n \"${LIGHT_SITE_USER:-}\" ]; then userdel \"$LIGHT_SITE_USER\" >/dev/null 2>&1 || true; fi".to_string(),
        "rm -f \"/etc/ssh/sshd_config.d/hosting-$site.conf\"".to_string(),
        "systemctl reload ssh >/dev/null 2>&1 || systemctl reload sshd >/dev/null 2>&1 || true".to_string(),
        "systemctl reload caddy >/dev/null 2>&1 || caddy reload --config /etc/caddy/Caddyfile >/dev/null 2>&1 || true".to_string(),
        if delete_volumes {
            "rm -rf \"$site_root\"".to_string()
        } else {
            [
                "mkdir -p \"$deleted_root\"",
                "if [ -d \"$site_root\" ]; then",
                "  mv \"$site_root\" \"$deleted_root/$site-$(date +%Y%m%d%H%M%S)\"",
                "fi",
            ]
            .join("\n")
        },
    ]
    .join("\n");

    let output = ssh
        .execute(&format!("bash -lc {}", sh_quote(&script)))
        .await?;
    if !output.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo eliminando '{}' del runtime lightweight: {}",
            site_name, output.stderr
        )));
    }
    Ok(())
}

async fn reconfigure_site(
    ssh: &SshClient,
    site_name: &str,
    fqdn: Option<&str>,
    access_user: Option<&str>,
    access_password: Option<&str>,
) -> std::result::Result<(), CoolifyError> {
    /* [245A-8] Reconfigure actualiza el contrato operativo minimo del sitio lightweight
     * sin recrearlo: dominio/Caddy y password SFTP. Cambiar access_user sigue bloqueado
     * porque implicaria rehacer ownership y jail del sitio en el host compartido. */
    let site_root = format!("/srv/hosting/{site_name}");
    let metadata_file = format!("{site_root}/site.env");
    let caddy_available = format!("/etc/caddy/sites-available/{site_name}.caddy");
    let caddy_enabled = format!("/etc/caddy/sites-enabled/{site_name}.caddy");
    let requested_fqdn = fqdn.map(str::trim).filter(|value| !value.is_empty());
    let requested_user = access_user.map(str::trim).filter(|value| !value.is_empty());
    let requested_password = access_password
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let script = vec![
        "set -euo pipefail".to_string(),
        format!("metadata_file={}", sh_quote(&metadata_file)),
        format!("caddy_available={}", sh_quote(&caddy_available)),
        format!("caddy_enabled={}", sh_quote(&caddy_enabled)),
        "if [ ! -f \"$metadata_file\" ]; then echo \"Metadatos del sitio no encontrados\" >&2; exit 14; fi".to_string(),
        ". \"$metadata_file\"".to_string(),
        match requested_fqdn {
            Some(value) => format!("next_fqdn={}", sh_quote(value)),
            None => "next_fqdn=\"$LIGHT_SITE_FQDN\"".to_string(),
        },
        match requested_user {
            Some(value) => format!("requested_user={}", sh_quote(value)),
            None => "requested_user=\"$LIGHT_SITE_USER\"".to_string(),
        },
        match requested_password {
            Some(value) => format!("requested_password={}", sh_quote(value)),
            None => "requested_password=''".to_string(),
        },
        "if [ \"$requested_user\" != \"$LIGHT_SITE_USER\" ]; then echo \"El runtime lightweight aun no soporta cambiar access_user\" >&2; exit 15; fi".to_string(),
        "if [ -n \"$requested_password\" ]; then printf '%s:%s\n' \"$LIGHT_SITE_USER\" \"$requested_password\" | chpasswd; fi".to_string(),
        "cat > \"$caddy_available\" <<EOF".to_string(),
        "$next_fqdn {".to_string(),
        "    encode zstd gzip".to_string(),
        "    reverse_proxy 127.0.0.1:$LIGHT_SITE_HTTP_PORT".to_string(),
        "    header {".to_string(),
        "        X-Content-Type-Options nosniff".to_string(),
        "        X-Frame-Options SAMEORIGIN".to_string(),
        "        Referrer-Policy strict-origin-when-cross-origin".to_string(),
        "    }".to_string(),
        "}".to_string(),
        "EOF".to_string(),
        "cat > \"$metadata_file\" <<EOF".to_string(),
        "LIGHT_SITE_USER=$LIGHT_SITE_USER".to_string(),
        "LIGHT_SITE_HTTP_PORT=$LIGHT_SITE_HTTP_PORT".to_string(),
        "LIGHT_SITE_FQDN=$next_fqdn".to_string(),
        "EOF".to_string(),
        "ln -sfn \"$caddy_available\" \"$caddy_enabled\"".to_string(),
        "caddy validate --config /etc/caddy/Caddyfile >/dev/null".to_string(),
        "systemctl reload caddy >/dev/null 2>&1 || caddy reload --config /etc/caddy/Caddyfile >/dev/null 2>&1".to_string(),
    ]
    .join("\n");

    let output = ssh
        .execute(&format!("bash -lc {}", sh_quote(&script)))
        .await?;
    if !output.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo reconfigurando '{}' del runtime lightweight: {}",
            site_name, output.stderr
        )));
    }
    Ok(())
}

async fn resolve_compose_file(
    ssh: &SshClient,
    site_name: &str,
) -> std::result::Result<String, CoolifyError> {
    let script = vec![
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

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn base64_encode(value: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(value)
}

fn generate_access_user(site_name: &str) -> String {
    let cleaned = site_name
        .chars()
        .filter(|char| char.is_ascii_alphanumeric())
        .take(12)
        .collect::<String>()
        .to_lowercase();
    let suffix: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(4)
        .map(char::from)
        .collect::<String>()
        .to_lowercase();
    format!("sftp_{}{}", cleaned, suffix)
}

fn generate_access_password() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(24)
        .map(char::from)
        .collect()
}

fn build_static_compose(http_port: u16) -> String {
    format!(
        "services:\n  web:\n    image: nginx:alpine\n    restart: unless-stopped\n    ports:\n      - '127.0.0.1:{http_port}:80'\n    volumes:\n      - ./public:/usr/share/nginx/html:ro\n"
    )
}

fn build_caddyfile(fqdn: &str, http_port: u16) -> String {
    format!(
        "{fqdn} {{\n    encode zstd gzip\n    reverse_proxy 127.0.0.1:{http_port}\n    header {{\n        X-Content-Type-Options nosniff\n        X-Frame-Options SAMEORIGIN\n        Referrer-Policy strict-origin-when-cross-origin\n    }}\n}}\n"
    )
}

fn build_sshd_match(site_name: &str, access_user: &str) -> String {
    format!(
        "Match User {access_user}\n    ChrootDirectory /srv/hosting/{site_name}\n    ForceCommand internal-sftp -d /public\n    PasswordAuthentication yes\n    AllowTcpForwarding no\n    X11Forwarding no\n"
    )
}

fn build_site_metadata(access_user: &str, http_port: u16, fqdn: &str) -> String {
    format!(
        "LIGHT_SITE_USER={access_user}\nLIGHT_SITE_HTTP_PORT={http_port}\nLIGHT_SITE_FQDN={fqdn}\n"
    )
}

#[allow(clippy::too_many_arguments)]
fn build_provision_static_script(
    project_root: &str,
    public_root: &str,
    compose_file: &str,
    metadata_file: &str,
    caddy_available: &str,
    caddy_enabled: &str,
    sshd_match_file: &str,
    access_user: &str,
    access_password: &str,
    compose_yaml: &str,
    metadata: &str,
    caddyfile: &str,
    sshd_match: &str,
    index_html: &str,
    http_port: u16,
) -> String {
    vec![
        "set -euo pipefail".to_string(),
        format!("site_root={}", sh_quote(project_root)),
        format!("public_root={}", sh_quote(public_root)),
        format!("compose_file={}", sh_quote(compose_file)),
        format!("metadata_file={}", sh_quote(metadata_file)),
        format!("caddy_available={}", sh_quote(caddy_available)),
        format!("caddy_enabled={}", sh_quote(caddy_enabled)),
        format!("sshd_match_file={}", sh_quote(sshd_match_file)),
        format!("access_user={}", sh_quote(access_user)),
        format!("access_password={}", sh_quote(access_password)),
        "if [ -e \"$site_root\" ]; then echo \"El sitio ya existe en $site_root\" >&2; exit 12; fi".to_string(),
        "if id \"$access_user\" >/dev/null 2>&1; then echo \"El usuario SFTP $access_user ya existe\" >&2; exit 13; fi".to_string(),
        "mkdir -p \"$site_root\" \"$public_root\" /etc/ssh/sshd_config.d".to_string(),
        "chown root:root \"$site_root\"".to_string(),
        "chmod 755 \"$site_root\"".to_string(),
        "useradd -d / -M -s /usr/sbin/nologin \"$access_user\"".to_string(),
        "printf '%s:%s\n' \"$access_user\" \"$access_password\" | chpasswd".to_string(),
        "chown \"$access_user:$access_user\" \"$public_root\"".to_string(),
        "chmod 755 \"$public_root\"".to_string(),
        format!(
            "printf '%s' {} | base64 -d > \"$compose_file\"",
            sh_quote(&base64_encode(compose_yaml))
        ),
        format!(
            "printf '%s' {} | base64 -d > \"$metadata_file\"",
            sh_quote(&base64_encode(metadata))
        ),
        format!(
            "printf '%s' {} | base64 -d > \"$caddy_available\"",
            sh_quote(&base64_encode(caddyfile))
        ),
        format!(
            "printf '%s' {} | base64 -d > \"$sshd_match_file\"",
            sh_quote(&base64_encode(sshd_match))
        ),
        format!(
            "printf '%s' {} | base64 -d > \"$public_root/index.html\"",
            sh_quote(&base64_encode(index_html))
        ),
        "ln -sfn \"$caddy_available\" \"$caddy_enabled\"".to_string(),
        "caddy validate --config /etc/caddy/Caddyfile >/dev/null".to_string(),
        "systemctl reload caddy >/dev/null 2>&1 || caddy reload --config /etc/caddy/Caddyfile >/dev/null 2>&1".to_string(),
        "systemctl reload ssh >/dev/null 2>&1 || systemctl reload sshd >/dev/null 2>&1 || true".to_string(),
        "compose() { if docker compose version >/dev/null 2>&1; then docker compose \"$@\"; elif command -v docker-compose >/dev/null 2>&1; then docker-compose \"$@\"; else echo \"docker compose no esta disponible\" >&2; exit 1; fi; }".to_string(),
        "compose -p \"$(basename \"$site_root\")\" -f \"$compose_file\" up -d".to_string(),
        format!("curl -fsS -o /dev/null http://127.0.0.1:{http_port}/"),
    ]
    .join("\n")
}

fn build_restore_script(
    site_name: &str,
    remote_artifact: &str,
    access_password: Option<&str>,
) -> String {
    let requested_password = access_password
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    vec![
        "set -euo pipefail".to_string(),
        "compose() { if docker compose version >/dev/null 2>&1; then docker compose \"$@\"; elif command -v docker-compose >/dev/null 2>&1; then docker-compose \"$@\"; else echo \"docker compose no esta disponible\" >&2; exit 1; fi; }".to_string(),
        format!("site={}", sh_quote(site_name)),
        format!("restore_archive={}", sh_quote(remote_artifact)),
        match requested_password {
            Some(value) => format!("requested_password={}", sh_quote(&value)),
            None => "requested_password=''".to_string(),
        },
        "site_root=\"/srv/hosting/$site\"".to_string(),
        "compose_file=''".to_string(),
        "for candidate in \"$site_root/docker-compose.yml\" \"$site_root/docker-compose.yaml\" \"$site_root/compose.yml\" \"$site_root/compose.yaml\"; do".to_string(),
        "  if [ -f \"$candidate\" ]; then compose_file=\"$candidate\"; break; fi".to_string(),
        "done".to_string(),
        "if [ -n \"$compose_file\" ]; then compose -p \"$site\" -f \"$compose_file\" down >/dev/null 2>&1 || true; fi".to_string(),
        "rm -rf \"$site_root\"".to_string(),
        "mkdir -p /srv/hosting /etc/caddy/sites-available /etc/caddy/sites-enabled /etc/ssh/sshd_config.d".to_string(),
        "tar -xzf \"$restore_archive\" -C /".to_string(),
        "metadata_file=\"$site_root/site.env\"".to_string(),
        "if [ ! -f \"$metadata_file\" ]; then echo \"Backup lightweight sin site.env\" >&2; exit 30; fi".to_string(),
        ". \"$metadata_file\"".to_string(),
        "if ! id \"$LIGHT_SITE_USER\" >/dev/null 2>&1; then".to_string(),
        "  useradd -d / -M -s /usr/sbin/nologin \"$LIGHT_SITE_USER\"".to_string(),
        "  if [ -z \"$requested_password\" ]; then requested_password=$(tr -dc 'A-Za-z0-9' </dev/urandom | head -c 24); fi".to_string(),
        "fi".to_string(),
        "if [ -n \"$requested_password\" ]; then printf '%s:%s\n' \"$LIGHT_SITE_USER\" \"$requested_password\" | chpasswd; fi".to_string(),
        "chown root:root \"$site_root\"".to_string(),
        "chmod 755 \"$site_root\"".to_string(),
        "if [ -d \"$site_root/public\" ]; then chown -R \"$LIGHT_SITE_USER:$LIGHT_SITE_USER\" \"$site_root/public\"; chmod 755 \"$site_root/public\"; fi".to_string(),
        "cat > \"/etc/caddy/sites-available/$site.caddy\" <<EOF".to_string(),
        "$LIGHT_SITE_FQDN {".to_string(),
        "    encode zstd gzip".to_string(),
        "    reverse_proxy 127.0.0.1:$LIGHT_SITE_HTTP_PORT".to_string(),
        "    header {".to_string(),
        "        X-Content-Type-Options nosniff".to_string(),
        "        X-Frame-Options SAMEORIGIN".to_string(),
        "        Referrer-Policy strict-origin-when-cross-origin".to_string(),
        "    }".to_string(),
        "}".to_string(),
        "EOF".to_string(),
        "cat > \"/etc/ssh/sshd_config.d/hosting-$site.conf\" <<EOF".to_string(),
        "Match User $LIGHT_SITE_USER".to_string(),
        "    ChrootDirectory /srv/hosting/$site".to_string(),
        "    ForceCommand internal-sftp -d /public".to_string(),
        "    PasswordAuthentication yes".to_string(),
        "    AllowTcpForwarding no".to_string(),
        "    X11Forwarding no".to_string(),
        "EOF".to_string(),
        "ln -sfn \"/etc/caddy/sites-available/$site.caddy\" \"/etc/caddy/sites-enabled/$site.caddy\"".to_string(),
        "caddy validate --config /etc/caddy/Caddyfile >/dev/null".to_string(),
        "systemctl reload caddy >/dev/null 2>&1 || caddy reload --config /etc/caddy/Caddyfile >/dev/null 2>&1".to_string(),
        "systemctl reload ssh >/dev/null 2>&1 || systemctl reload sshd >/dev/null 2>&1 || true".to_string(),
        "compose_file=''".to_string(),
        "for candidate in \"$site_root/docker-compose.yml\" \"$site_root/docker-compose.yaml\" \"$site_root/compose.yml\" \"$site_root/compose.yaml\"; do".to_string(),
        "  if [ -f \"$candidate\" ]; then compose_file=\"$candidate\"; break; fi".to_string(),
        "done".to_string(),
        "if [ -z \"$compose_file\" ]; then echo \"Backup lightweight sin compose\" >&2; exit 31; fi".to_string(),
        "compose -p \"$site\" -f \"$compose_file\" up -d".to_string(),
        "curl -fsS -o /dev/null \"http://127.0.0.1:$LIGHT_SITE_HTTP_PORT/\"".to_string(),
        "printf 'RESTORE_FQDN=%s\n' \"$LIGHT_SITE_FQDN\"".to_string(),
        "printf 'RESTORE_ACCESS_USER=%s\n' \"$LIGHT_SITE_USER\"".to_string(),
        "if [ -n \"$requested_password\" ]; then printf 'RESTORE_ACCESS_PASSWORD=%s\n' \"$requested_password\"; fi".to_string(),
    ]
    .join("\n")
}

fn parse_restore_output(raw: &str) -> RestoreOutputMetadata {
    let mut output = RestoreOutputMetadata::default();

    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("RESTORE_FQDN=") {
            output.fqdn = (!value.trim().is_empty()).then(|| value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("RESTORE_ACCESS_USER=") {
            output.access_user = (!value.trim().is_empty()).then(|| value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("RESTORE_ACCESS_PASSWORD=") {
            output.access_password = (!value.trim().is_empty()).then(|| value.trim().to_string());
        }
    }

    output
}

fn build_backup_id(label: Option<&str>) -> String {
    let base = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    match label {
        Some(value) if !value.trim().is_empty() => {
            format!("{}-{}", base, sanitize_path_name(value))
        }
        _ => base,
    }
}

fn sanitize_path_name(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' => character,
            _ => '_',
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn retention_keep_for(tier: &BackupTier) -> Option<usize> {
    match tier {
        BackupTier::Daily => Some(3),
        BackupTier::Weekly => Some(2),
        BackupTier::Manual => None,
    }
}

fn build_local_artifact(
    kind: &str,
    logical_name: &str,
    file_path: &Path,
    original_path: Option<String>,
) -> std::result::Result<BackupArtifact, CoolifyError> {
    let bytes = fs::read(file_path)?;
    Ok(BackupArtifact {
        kind: kind.to_string(),
        logical_name: logical_name.to_string(),
        relative_path: file_path
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .ok_or_else(|| {
                CoolifyError::Validation(format!(
                    "Artifacto lightweight sin file_name: {}",
                    file_path.display()
                ))
            })?,
        original_path,
        size_bytes: bytes.len() as u64,
        sha256: hash_bytes(&bytes),
    })
}

fn write_local_manifest(
    directory: &Path,
    manifest: &BackupManifest,
) -> std::result::Result<(), CoolifyError> {
    let json = serde_json::to_string_pretty(manifest)
        .map_err(|error| CoolifyError::Validation(error.to_string()))?;
    fs::write(directory.join("manifest.json"), json)?;
    Ok(())
}

fn read_local_manifest(path: &Path) -> std::result::Result<BackupManifest, CoolifyError> {
    let content = fs::read_to_string(path)?;
    serde_json::from_str(&content)
        .map_err(|error| CoolifyError::Validation(format!("Manifiesto inválido: {error}")))
}

fn validate_local_manifest(
    directory: &Path,
    manifest: &BackupManifest,
) -> std::result::Result<(), CoolifyError> {
    if manifest.artifacts.is_empty() {
        return Err(CoolifyError::Validation(
            "Backup lightweight sin artifacts".to_string(),
        ));
    }

    for artifact in &manifest.artifacts {
        let artifact_path = directory.join(&artifact.relative_path);
        let bytes = fs::read(&artifact_path)?;
        if hash_bytes(&bytes) != artifact.sha256 {
            return Err(CoolifyError::Validation(format!(
                "Checksum inválido en {}",
                artifact.relative_path
            )));
        }
    }

    Ok(())
}

fn create_local_archive(
    source_dir: &Path,
    archive_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    let folder_name = source_dir
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            CoolifyError::Validation(format!(
                "Ruta de backup lightweight inválida: {}",
                source_dir.display()
            ))
        })?;
    let archive_file = File::create(archive_path)?;
    let encoder = GzEncoder::new(archive_file, Compression::default());
    let mut builder = Builder::new(encoder);
    builder.append_dir_all(folder_name, source_dir)?;
    builder.finish()?;
    Ok(())
}

fn cleanup_dir(path: &Path) -> std::result::Result<(), std::io::Error> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn build_default_index(site_name: &str, fqdn: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"es\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n  <title>{site_name}</title>\n  <style>body{{font-family:system-ui,sans-serif;margin:0;padding:3rem;background:#f6f4ef;color:#1f2937}}main{{max-width:48rem;margin:0 auto}}h1{{font-size:2rem;margin-bottom:0.5rem}}p{{line-height:1.6}}</style>\n</head>\n<body>\n  <main>\n    <h1>{site_name}</h1>\n    <p>Tu hosting lightweight ya está operativo en {fqdn}.</p>\n    <p>Sube tus archivos por SFTP al directorio <strong>/public</strong>.</p>\n  </main>\n</body>\n</html>\n"
    )
}

async fn allocate_http_port(ssh: &SshClient) -> std::result::Result<u16, CoolifyError> {
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

    #[test]
    fn lightweight_action_parse_rejects_unknown_values() {
        assert_eq!(
            LightweightSiteAction::parse("restart").unwrap(),
            LightweightSiteAction::Restart
        );
        assert_eq!(
            LightweightSiteAction::parse("reconfigure").unwrap(),
            LightweightSiteAction::Reconfigure
        );
        assert!(LightweightSiteAction::parse("rotate").is_err());
    }

    #[test]
    fn parse_restore_output_extracts_runtime_fields() {
        let parsed = parse_restore_output(
            "RESTORE_FQDN=demo.example.com\nRESTORE_ACCESS_USER=sftp_demo\nRESTORE_ACCESS_PASSWORD=secret123\n",
        );
        assert_eq!(parsed.fqdn.as_deref(), Some("demo.example.com"));
        assert_eq!(parsed.access_user.as_deref(), Some("sftp_demo"));
        assert_eq!(parsed.access_password.as_deref(), Some("secret123"));
    }
}