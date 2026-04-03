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

fn default_upload_max() -> String {
    "64M".to_string()
}
fn default_post_max() -> String {
    "70M".to_string()
}
fn default_memory_limit() -> String {
    "1G".to_string()
}

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

fn default_smtp_port() -> u16 {
    587
}
fn default_smtp_from_name() -> String {
    "Kamples".to_string()
}
fn default_smtp_secure() -> String {
    "tls".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BackupTier {
    Daily,
    Weekly,
    #[default]
    Manual,
}

impl std::fmt::Display for BackupTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Daily => write!(f, "daily"),
            Self::Weekly => write!(f, "weekly"),
            Self::Manual => write!(f, "manual"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupPolicy {
    #[serde(default = "default_backup_enabled")]
    pub enabled: bool,
    #[serde(rename = "dailyKeep", default = "default_daily_keep")]
    pub daily_keep: usize,
    #[serde(rename = "weeklyKeep", default = "default_weekly_keep")]
    pub weekly_keep: usize,
    #[serde(rename = "sourcePaths", default)]
    pub source_paths: Vec<String>,
}

impl Default for BackupPolicy {
    fn default() -> Self {
        Self {
            enabled: default_backup_enabled(),
            daily_keep: default_daily_keep(),
            weekly_keep: default_weekly_keep(),
            source_paths: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    #[serde(rename = "httpPath", default = "default_health_path")]
    pub http_path: String,
    #[serde(rename = "timeoutSeconds", default = "default_health_timeout")]
    pub timeout_seconds: u64,
    #[serde(rename = "fatalPatterns", default = "default_fatal_patterns")]
    pub fatal_patterns: Vec<String>,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            http_path: default_health_path(),
            timeout_seconds: default_health_timeout(),
            fatal_patterns: default_fatal_patterns(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseEngine {
    Mariadb,
    Postgres,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum DnsRecordType {
    A,
    AAAA,
    CNAME,
}

impl std::fmt::Display for DnsRecordType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::A => write!(f, "A"),
            Self::AAAA => write!(f, "AAAA"),
            Self::CNAME => write!(f, "CNAME"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteDnsRecord {
    #[serde(default = "default_dns_record_name")]
    pub name: String,
    #[serde(rename = "type", default = "default_dns_record_type")]
    pub record_type: DnsRecordType,
    #[serde(default = "default_dns_ttl")]
    pub ttl: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteDnsConfig {
    pub provider: String,
    pub zone: String,
    #[serde(rename = "switchOnMigration", default = "default_switch_on_migration")]
    pub switch_on_migration: bool,
    #[serde(default)]
    pub records: Vec<SiteDnsRecord>,
}

fn default_backup_enabled() -> bool {
    true
}
fn default_daily_keep() -> usize {
    2
}
fn default_weekly_keep() -> usize {
    3
}
fn default_health_path() -> String {
    "/".to_string()
}
fn default_health_timeout() -> u64 {
    20
}
fn default_dns_record_name() -> String {
    "@".to_string()
}
fn default_dns_record_type() -> DnsRecordType {
    DnsRecordType::A
}
fn default_dns_ttl() -> u32 {
    300
}
fn default_switch_on_migration() -> bool {
    true
}
fn default_fatal_patterns() -> Vec<String> {
    vec![
        "Fatal error".to_string(),
        "Uncaught Error".to_string(),
        "There has been a critical error".to_string(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteConfig {
    pub nombre: String,
    pub dominio: String,
    #[serde(default)]
    pub target: Option<String>,
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
    /* [044A-1] URL del repositorio git para stacks Rust (template rendering) */
    #[serde(rename = "repoUrl", default)]
    pub repo_url: Option<String>,
    #[serde(rename = "backupPolicy", default)]
    pub backup_policy: BackupPolicy,
    #[serde(rename = "healthCheck", default)]
    pub health_check: HealthCheckConfig,
    #[serde(rename = "dnsConfig", default)]
    pub dns_config: Option<SiteDnsConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StackTemplate {
    #[default]
    Wordpress,
    Kamples,
    Minecraft,
    Rust,
}

impl std::fmt::Display for StackTemplate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wordpress => write!(f, "wordpress"),
            Self::Kamples => write!(f, "kamples"),
            Self::Minecraft => write!(f, "minecraft"),
            Self::Rust => write!(f, "rust"),
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
        let json =
            r#"{"serverName": "survival", "memory": "3G", "maxPlayers": 10, "difficulty": 2}"#;
        let mc: MinecraftServer = serde_json::from_str(json).unwrap();
        assert_eq!(mc.server_name, "survival");
        assert_eq!(mc.memory, "3G");
        assert_eq!(mc.max_players, 10);
        assert!(mc.stack_uuid.is_none());
    }

    #[test]
    fn test_command_output_success() {
        let ok = CommandOutput {
            stdout: "ok".into(),
            stderr: String::new(),
            exit_code: 0,
        };
        assert!(ok.success());

        let fail = CommandOutput {
            stdout: String::new(),
            stderr: "error".into(),
            exit_code: 1,
        };
        assert!(!fail.success());
    }

    #[test]
    fn test_stack_template_display() {
        assert_eq!(StackTemplate::Wordpress.to_string(), "wordpress");
        assert_eq!(StackTemplate::Kamples.to_string(), "kamples");
        assert_eq!(StackTemplate::Minecraft.to_string(), "minecraft");
        assert_eq!(StackTemplate::Rust.to_string(), "rust");
    }

    #[test]
    fn test_backup_policy_defaults() {
        let json = r#"{"nombre": "blog", "dominio": "https://blog.com"}"#;
        let site: SiteConfig = serde_json::from_str(json).unwrap();
        assert!(site.backup_policy.enabled);
        assert_eq!(site.backup_policy.daily_keep, 2);
        assert_eq!(site.backup_policy.weekly_keep, 3);
        assert_eq!(site.health_check.http_path, "/");
        assert!(site.target.is_none());
        assert!(site.dns_config.is_none());
    }
}
