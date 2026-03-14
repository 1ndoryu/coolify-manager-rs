/*
 * Tipos de dominio para sitios, stacks, servidores y templates.
 * Representan la estructura de datos del negocio, desacoplados de infra.
 */

use serde::{Deserialize, Serialize};

/// Configuracion de PHP por tema. Se escribe como ini en conf.d del contenedor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhpConfig {
    #[serde(rename = "uploadMaxFilesize", default = "default_upload_max")]
    pub upload_max_filesize: String,
    #[serde(rename = "postMaxSize", default = "default_post_max")]
    pub post_max_size: String,
    #[serde(rename = "memoryLimit", default = "default_memory_limit")]
    pub memory_limit: String,
}

impl Default for PhpConfig {
    fn default() -> Self {
        Self {
            upload_max_filesize: default_upload_max(),
            post_max_size: default_post_max(),
            memory_limit: default_memory_limit(),
        }
    }
}

fn default_upload_max() -> String { "64M".to_string() }
fn default_post_max() -> String { "70M".to_string() }
fn default_memory_limit() -> String { "1G".to_string() }

/// Configuracion SMTP para wp_mail. Se despliega como mu-plugin que configura PHPMailer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmtpConfig {
    pub host: String,
    #[serde(default = "default_smtp_port")]
    pub port: u16,
    pub user: String,
    pub password: String,
    #[serde(rename = "fromEmail")]
    pub from_email: String,
    #[serde(rename = "fromName", default = "default_smtp_from_name")]
    pub from_name: String,
    #[serde(default = "default_smtp_secure")]
    pub secure: String, /* tls | ssl | none */
}

fn default_smtp_port() -> u16 { 587 }
fn default_smtp_from_name() -> String { "Kamples".to_string() }
fn default_smtp_secure() -> String { "tls".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteConfig {
    pub nombre: String,
    pub dominio: String,
    #[serde(rename = "stackUuid", default)]
    pub stack_uuid: Option<String>,
    #[serde(rename = "gloryBranch", default = "default_branch")]
    pub glory_branch: String,
    #[serde(rename = "libraryBranch", default = "default_branch")]
    pub library_branch: String,
    #[serde(rename = "themeName", default = "default_theme_name")]
    pub theme_name: String,
    #[serde(rename = "skipReact", default)]
    pub skip_react: bool,
    #[serde(default = "default_template")]
    pub template: StackTemplate,
    #[serde(rename = "phpConfig", default)]
    pub php_config: Option<PhpConfig>,
    #[serde(rename = "smtpConfig", default)]
    pub smtp_config: Option<SmtpConfig>,
    #[serde(rename = "disableWpCron", default)]
    pub disable_wp_cron: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StackTemplate {
    Wordpress,
    Kamples,
    Minecraft,
}

impl Default for StackTemplate {
    fn default() -> Self {
        Self::Wordpress
    }
}

impl std::fmt::Display for StackTemplate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wordpress => write!(f, "wordpress"),
            Self::Kamples => write!(f, "kamples"),
            Self::Minecraft => write!(f, "minecraft"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftServer {
    #[serde(rename = "serverName")]
    pub server_name: String,
    #[serde(rename = "stackUuid", default)]
    pub stack_uuid: Option<String>,
    #[serde(default = "default_mc_memory")]
    pub memory: String,
    #[serde(rename = "maxPlayers", default = "default_mc_players")]
    pub max_players: u32,
    #[serde(default = "default_mc_difficulty")]
    pub difficulty: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub uuid: String,
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub fqdn: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl CommandOutput {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

#[derive(Debug, Clone)]
pub struct ContainerFilter {
    pub stack_uuid: Option<String>,
    pub name_contains: Option<String>,
    pub image_contains: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackCreationResult {
    pub uuid: String,
    pub name: String,
}

/* Defaults para serde */
fn default_branch() -> String {
    "main".to_string()
}

fn default_theme_name() -> String {
    "glory".to_string()
}

fn default_template() -> StackTemplate {
    StackTemplate::Wordpress
}

fn default_mc_memory() -> String {
    "1536M".to_string()
}

fn default_mc_players() -> u32 {
    20
}

fn default_mc_difficulty() -> u32 {
    2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_site_config_deserialize_defaults() {
        let json = r#"{"nombre": "blog", "dominio": "https://blog.com"}"#;
        let site: SiteConfig = serde_json::from_str(json).unwrap();
        assert_eq!(site.nombre, "blog");
        assert_eq!(site.dominio, "https://blog.com");
        assert_eq!(site.glory_branch, "main");
        assert_eq!(site.library_branch, "main");
        assert_eq!(site.theme_name, "glory");
        assert!(!site.skip_react);
        assert_eq!(site.template, StackTemplate::Wordpress);
        assert!(site.stack_uuid.is_none());
    }

    #[test]
    fn test_site_config_deserialize_full() {
        let json = r#"{
            "nombre": "cap",
            "dominio": "https://cap.wandori.us",
            "stackUuid": "zkcc040cc0scock4kcooowkc",
            "gloryBranch": "ecommerce",
            "libraryBranch": "main",
            "themeName": "glorytemplate",
            "skipReact": true,
            "template": "kamples"
        }"#;
        let site: SiteConfig = serde_json::from_str(json).unwrap();
        assert_eq!(site.nombre, "cap");
        assert_eq!(site.stack_uuid.as_deref(), Some("zkcc040cc0scock4kcooowkc"));
        assert_eq!(site.glory_branch, "ecommerce");
        assert!(site.skip_react);
        assert_eq!(site.template, StackTemplate::Kamples);
    }

    #[test]
    fn test_minecraft_server_deserialize() {
        let json = r#"{"serverName": "survival", "memory": "3G", "maxPlayers": 10, "difficulty": 2}"#;
        let mc: MinecraftServer = serde_json::from_str(json).unwrap();
        assert_eq!(mc.server_name, "survival");
        assert_eq!(mc.memory, "3G");
        assert_eq!(mc.max_players, 10);
        assert!(mc.stack_uuid.is_none());
    }

    #[test]
    fn test_command_output_success() {
        let ok = CommandOutput { stdout: "ok".into(), stderr: String::new(), exit_code: 0 };
        assert!(ok.success());

        let fail = CommandOutput { stdout: String::new(), stderr: "error".into(), exit_code: 1 };
        assert!(!fail.success());
    }

    #[test]
    fn test_stack_template_display() {
        assert_eq!(StackTemplate::Wordpress.to_string(), "wordpress");
        assert_eq!(StackTemplate::Kamples.to_string(), "kamples");
        assert_eq!(StackTemplate::Minecraft.to_string(), "minecraft");
    }
}
