use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;
use crate::services::backup_manager;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    backup_id: &str,
    skip_safety_snapshot: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;
    let target = settings.resolve_site_target(site)?;

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    backup_manager::restore_site_backup(
        &settings,
        config_path,
        site,
        &ssh,
        backup_id,
        skip_safety_snapshot,
    )
    .await?;
    println!("Backup restaurado: {backup_id}");
    Ok(())
}
