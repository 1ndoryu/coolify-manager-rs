use crate::domain::{DatabaseEngine, SiteConfig, StackTemplate};
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;

#[derive(Debug, Clone)]
pub struct DatabaseBinding {
    pub logical_name: &'static str,
    pub engine: DatabaseEngine,
    pub name_hint: &'static str,
    pub image_hint: &'static str,
}

#[derive(Debug, Clone)]
pub struct SiteCapabilities {
    pub app_name_hint: &'static str,
    pub app_image_hint: &'static str,
    pub persistent_paths: Vec<String>,
    pub database_bindings: Vec<DatabaseBinding>,
    pub supports_theme_git: bool,
}

impl SiteCapabilities {
    pub async fn resolve_app_container(
        &self,
        ssh: &SshClient,
        stack_uuid: &str,
    ) -> std::result::Result<String, CoolifyError> {
        docker::find_container(
            ssh,
            &crate::domain::ContainerFilter {
                stack_uuid: Some(stack_uuid.to_string()),
                name_contains: Some(self.app_name_hint.to_string()),
                image_contains: Some(self.app_image_hint.to_string()),
            },
        )
        .await
    }

    pub async fn resolve_database_container(
        &self,
        ssh: &SshClient,
        stack_uuid: &str,
        binding: &DatabaseBinding,
    ) -> std::result::Result<String, CoolifyError> {
        docker::find_container(
            ssh,
            &crate::domain::ContainerFilter {
                stack_uuid: Some(stack_uuid.to_string()),
                name_contains: Some(binding.name_hint.to_string()),
                image_contains: Some(binding.image_hint.to_string()),
            },
        )
        .await
    }

    pub fn theme_directory(&self, site: &SiteConfig) -> Option<String> {
        if self.supports_theme_git {
            Some(format!(
                "/var/www/html/wp-content/themes/{}",
                site.theme_name
            ))
        } else {
            None
        }
    }

    pub fn health_url(&self, site: &SiteConfig) -> String {
        let domain = site.dominio.trim_end_matches('/');
        let path = site.health_check.http_path.trim();
        if path.is_empty() || path == "/" {
            domain.to_string()
        } else {
            format!("{domain}/{}", path.trim_start_matches('/'))
        }
    }
}

pub fn resolve(site: &SiteConfig) -> SiteCapabilities {
    match site.template {
        StackTemplate::Wordpress => SiteCapabilities {
            app_name_hint: "wordpress",
            app_image_hint: "wordpress",
            persistent_paths: default_wordpress_paths(site),
            database_bindings: vec![DatabaseBinding {
                logical_name: "wordpress",
                engine: DatabaseEngine::Mariadb,
                name_hint: "mariadb",
                image_hint: "mariadb",
            }],
            supports_theme_git: true,
        },
        StackTemplate::Kamples => SiteCapabilities {
            app_name_hint: "wordpress",
            app_image_hint: "wordpress",
            persistent_paths: default_wordpress_paths(site),
            database_bindings: vec![
                DatabaseBinding {
                    logical_name: "wordpress",
                    engine: DatabaseEngine::Mariadb,
                    name_hint: "mariadb",
                    image_hint: "mariadb",
                },
                DatabaseBinding {
                    logical_name: "kamples",
                    engine: DatabaseEngine::Postgres,
                    name_hint: "postgres",
                    image_hint: "postgres",
                },
            ],
            supports_theme_git: true,
        },
        StackTemplate::Minecraft => SiteCapabilities {
            app_name_hint: "minecraft",
            app_image_hint: "itzg/minecraft-server",
            persistent_paths: if site.backup_policy.source_paths.is_empty() {
                vec!["/data".to_string()]
            } else {
                site.backup_policy.source_paths.clone()
            },
            database_bindings: Vec::new(),
            supports_theme_git: false,
        },
    }
}

fn default_wordpress_paths(site: &SiteConfig) -> Vec<String> {
    if site.backup_policy.source_paths.is_empty() {
        vec!["/var/www/html/wp-content".to_string()]
    } else {
        site.backup_policy.source_paths.clone()
    }
}
