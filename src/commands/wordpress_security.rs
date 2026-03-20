use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;
use crate::infra::template_engine;
use crate::infra::validation;
use crate::services::wordpress_security_manager;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    audit: bool,
    user: Option<&str>,
    password: Option<&str>,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;
    let stack_uuid = site.stack_uuid.as_deref().unwrap();
    let target = settings.resolve_site_target(site)?;

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;
    let wp_container = docker::find_wordpress_container(&ssh, stack_uuid).await?;

    if audit || user.is_none() {
        let report =
            wordpress_security_manager::audit_wordpress_security(&ssh, &wp_container).await?;
        println!("debug_enabled={} file_editor_disabled={} force_ssl_admin={} default_admin={} admins={}", report.debug_enabled, report.file_editor_disabled, report.force_ssl_admin, report.has_default_admin_username, report.administrator_count);
        for recommendation in report.recommendations {
            println!("- {recommendation}");
        }
    }

    if let Some(username) = user {
        let generated_password;
        let password = match password {
            Some(value) => value,
            None => {
                generated_password = template_engine::generate_password(24);
                generated_password.as_str()
            }
        };
        wordpress_security_manager::rotate_admin_password(&ssh, &wp_container, username, password)
            .await?;
        println!(
            "Password admin actualizada para '{}': {}",
            username, password
        );
    }

    Ok(())
}
