/* Entidades de configuracion (split WIP de config/mod.rs).
 * Todos los *Config subordinados de Settings con sus defaults serde.
 */

use crate::domain::SmtpConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupStorageConfig {
    #[serde(rename = "localDir", default = "default_backup_local_dir")]
    pub local_dir: String,
    #[serde(rename = "remote", default)]
    pub remote: Option<RemoteBackupConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RemoteBackupConfig {
    GoogleDrive(GoogleDriveBackupConfig),
    /* [N1] Backup remoto via SSH/SCP a un segundo VPS. Reemplaza Google Drive. */
    SshRemote(SshRemoteBackupConfig),
}

/* [N1] Configuracion de backup remoto via SSH.
 * Estructura en el VPS remoto: {base_dir}/{site_name}/{tier}/{backup_id}.tar.gz
 *
 * directTransferKey: ruta a clave SSH EN VPS1 para transferir directamente a VPS2
 * sin pasar datos por el PC local. Cuando esta configurado, el backup se crea
 * enteramente en VPS1 y se envia a VPS2 por SCP a velocidad de datacenter (~100 Mbps)
 * en lugar de pasar por el internet domestico (~2 Mbps upload). */
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshRemoteBackupConfig {
    pub host: String,
    pub user: String,
    #[serde(rename = "sshKey", default)]
    pub ssh_key: Option<String>,
    #[serde(rename = "sshPassword", default)]
    pub ssh_password: Option<String>,
    #[serde(rename = "baseDir", default = "default_ssh_backup_base_dir")]
    pub base_dir: String,
    #[serde(rename = "directTransferKey", default)]
    pub direct_transfer_key: Option<String>,
}

fn default_ssh_backup_base_dir() -> String {
    "/backups/coolify-manager".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleDriveBackupConfig {
    #[serde(rename = "rootFolderId")]
    pub root_folder_id: String,
    #[serde(rename = "credentialsPath", default)]
    pub credentials_path: String,
    #[serde(rename = "serviceAccountEmail", default)]
    pub service_account_email: Option<String>,
    #[serde(rename = "oauthClientId", default)]
    pub oauth_client_id: Option<String>,
    #[serde(rename = "oauthClientSecret", default)]
    pub oauth_client_secret: Option<String>,
    #[serde(rename = "oauthRefreshToken", default)]
    pub oauth_refresh_token: Option<String>,
}

impl Default for BackupStorageConfig {
    fn default() -> Self {
        Self {
            local_dir: default_backup_local_dir(),
            remote: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentTargetConfig {
    pub name: String,
    pub vps: VpsConfig,
    pub coolify: CoolifyConfig,
    #[serde(rename = "maintenancePolicy", default)]
    pub maintenance_policy: Option<MaintenancePolicyConfig>,
    #[serde(rename = "securityPolicy", default)]
    pub security_policy: Option<SecurityPolicyConfig>,
    #[serde(rename = "hostProfile", default)]
    pub host_profile: Option<HostProfileConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenancePolicyConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub timezone: String,
    #[serde(
        rename = "windowStartLocal",
        default = "default_maintenance_window_start"
    )]
    pub window_start_local: String,
    #[serde(
        rename = "randomizedDelay",
        default = "default_maintenance_randomized_delay"
    )]
    pub randomized_delay: String,
    #[serde(
        rename = "durationBudget",
        default = "default_maintenance_duration_budget"
    )]
    pub duration_budget: String,
    #[serde(rename = "rebootPolicy", default)]
    pub reboot_policy: RebootPolicy,
    #[serde(
        rename = "maxRebootFrequency",
        default = "default_max_reboot_frequency"
    )]
    pub max_reboot_frequency: String,
    #[serde(rename = "sampleSites", default)]
    pub sample_sites: Vec<String>,
    #[serde(rename = "driftRules", default)]
    pub drift_rules: DriftRulesConfig,
}

impl Default for MaintenancePolicyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timezone: String::new(),
            window_start_local: default_maintenance_window_start(),
            randomized_delay: default_maintenance_randomized_delay(),
            duration_budget: default_maintenance_duration_budget(),
            reboot_policy: RebootPolicy::default(),
            max_reboot_frequency: default_max_reboot_frequency(),
            sample_sites: Vec::new(),
            drift_rules: DriftRulesConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RebootPolicy {
    #[default]
    IfRequired,
    IfDriftDetected,
    ManualOnly,
}

impl std::fmt::Display for RebootPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IfRequired => write!(f, "if-required"),
            Self::IfDriftDetected => write!(f, "if-drift-detected"),
            Self::ManualOnly => write!(f, "manual-only"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftRulesConfig {
    #[serde(
        rename = "requiredConsecutiveSnapshots",
        default = "default_required_consecutive_snapshots"
    )]
    pub required_consecutive_snapshots: u8,
    #[serde(
        rename = "avg15GreaterThanCpuCount",
        default = "default_avg15_greater_than_cpu_count"
    )]
    pub avg15_greater_than_cpu_count: bool,
    #[serde(
        rename = "controlPlaneCpuPercent",
        default = "default_control_plane_cpu_percent"
    )]
    pub control_plane_cpu_percent: f32,
    #[serde(
        rename = "controlPlaneCpuMultiplierVsBaseline",
        default = "default_control_plane_cpu_multiplier_vs_baseline"
    )]
    pub control_plane_cpu_multiplier_vs_baseline: f32,
    #[serde(rename = "cpuPsiSomeAvg10", default = "default_cpu_psi_some_avg10")]
    pub cpu_psi_some_avg10: f32,
    #[serde(rename = "ioPsiFullAvg10", default = "default_io_psi_full_avg10")]
    pub io_psi_full_avg10: f32,
}

impl Default for DriftRulesConfig {
    fn default() -> Self {
        Self {
            required_consecutive_snapshots: default_required_consecutive_snapshots(),
            avg15_greater_than_cpu_count: default_avg15_greater_than_cpu_count(),
            control_plane_cpu_percent: default_control_plane_cpu_percent(),
            control_plane_cpu_multiplier_vs_baseline:
                default_control_plane_cpu_multiplier_vs_baseline(),
            cpu_psi_some_avg10: default_cpu_psi_some_avg10(),
            io_psi_full_avg10: default_io_psi_full_avg10(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityPolicyConfig {
    #[serde(default)]
    pub ssh: Option<SshSecurityPolicyConfig>,
    #[serde(default)]
    pub firewall: Option<FirewallSecurityPolicyConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshSecurityPolicyConfig {
    #[serde(rename = "allowRootKeyOnly", default)]
    pub allow_root_key_only: bool,
    #[serde(rename = "disablePasswordAuth", default)]
    pub disable_password_auth: bool,
    #[serde(rename = "trustedSourceIps", default)]
    pub trusted_source_ips: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallSecurityPolicyConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(rename = "allowedTcpPorts", default)]
    pub allowed_tcp_ports: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostProfileConfig {
    #[serde(rename = "swapGb", default = "default_host_swap_gb")]
    pub swap_gb: u16,
    #[serde(rename = "swappiness", default = "default_host_swappiness")]
    pub swappiness: u8,
    #[serde(
        rename = "vfsCachePressure",
        default = "default_host_vfs_cache_pressure"
    )]
    pub vfs_cache_pressure: u16,
    #[serde(
        rename = "overcommitMemory",
        default = "default_host_overcommit_memory"
    )]
    pub overcommit_memory: u8,
    #[serde(rename = "disableThp", default = "default_host_disable_thp")]
    pub disable_thp: bool,
    #[serde(rename = "dockerLiveRestore", default)]
    pub docker_live_restore: bool,
}

impl Default for HostProfileConfig {
    fn default() -> Self {
        Self {
            swap_gb: default_host_swap_gb(),
            swappiness: default_host_swappiness(),
            vfs_cache_pressure: default_host_vfs_cache_pressure(),
            overcommit_memory: default_host_overcommit_memory(),
            disable_thp: default_host_disable_thp(),
            docker_live_restore: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsProviderConfig {
    pub name: String,
    #[serde(flatten)]
    pub provider: DnsProviderKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DnsProviderKind {
    Contabo(ContaboDnsConfig),
    Cloudflare(CloudflareDnsConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContaboDnsConfig {
    #[serde(rename = "clientId")]
    pub client_id: String,
    #[serde(rename = "clientSecret")]
    pub client_secret: String,
    pub username: String,
    #[serde(rename = "apiPassword")]
    pub api_password: String,
    #[serde(rename = "apiBaseUrl", default = "default_contabo_api_base_url")]
    pub api_base_url: String,
    #[serde(rename = "authBaseUrl", default = "default_contabo_auth_base_url")]
    pub auth_base_url: String,
}

/* [156A-1] Configuracion Cloudflare DNS.
 * Usa API Token (recomendado) en lugar de Global API Key.
 * Permisos requeridos en el token: Zone:DNS:Edit, Zone:Zone:Read.
 * proxy_default: si true, los nuevos registros se crean con proxy naranja activado. */
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudflareDnsConfig {
    #[serde(rename = "apiToken")]
    pub api_token: String,
    #[serde(rename = "proxyDefault", default)]
    pub proxy_default: bool,
}

/// Configuracion SMTP global del settings.json (formato legacy compatible).
/// El campo `user` actua tambien como direccion de origen cuando no hay `fromEmail`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmtpGlobalConfig {
    pub host: String,
    #[serde(default = "default_smtp_port")]
    pub port: u16,
    pub user: String,
    pub password: String,
    #[serde(rename = "fromName", default = "default_smtp_from_name")]
    pub from_name: String,
    #[serde(default = "default_smtp_secure")]
    pub secure: String,
}

fn default_smtp_port() -> u16 {
    587
}
fn default_smtp_from_name() -> String {
    "WordPress".to_string()
}
fn default_smtp_secure() -> String {
    "tls".to_string()
}
fn default_backup_local_dir() -> String {
    "backups".to_string()
}
fn default_contabo_api_base_url() -> String {
    "https://api.contabo.com".to_string()
}
fn default_contabo_auth_base_url() -> String {
    "https://auth.contabo.com/auth/realms/contabo/protocol/openid-connect/token".to_string()
}
fn default_maintenance_window_start() -> String {
    "03:00:00".to_string()
}
fn default_maintenance_randomized_delay() -> String {
    "15m".to_string()
}
fn default_maintenance_duration_budget() -> String {
    "45m".to_string()
}
fn default_max_reboot_frequency() -> String {
    "weekly".to_string()
}
fn default_required_consecutive_snapshots() -> u8 {
    3
}
fn default_avg15_greater_than_cpu_count() -> bool {
    true
}
fn default_control_plane_cpu_percent() -> f32 {
    35.0
}
fn default_control_plane_cpu_multiplier_vs_baseline() -> f32 {
    2.0
}
fn default_cpu_psi_some_avg10() -> f32 {
    25.0
}
fn default_io_psi_full_avg10() -> f32 {
    1.0
}
fn default_host_swap_gb() -> u16 {
    4
}
fn default_host_swappiness() -> u8 {
    10
}
fn default_host_vfs_cache_pressure() -> u16 {
    50
}
fn default_host_overcommit_memory() -> u8 {
    1
}
fn default_host_disable_thp() -> bool {
    true
}

impl SmtpGlobalConfig {
    pub fn as_smtp_config(&self) -> SmtpConfig {
        SmtpConfig {
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            password: self.password.clone(),
            from_email: self.user.clone(),
            from_name: self.from_name.clone(),
            secure: self.secure.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpsConfig {
    pub ip: String,
    pub user: String,
    #[serde(rename = "sshKey", default)]
    pub ssh_key: Option<String>,
    #[serde(rename = "sshPassword", default)]
    pub ssh_password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoolifyConfig {
    #[serde(rename = "baseUrl")]
    pub base_url: String,
    #[serde(rename = "apiToken")]
    pub api_token: String,
    #[serde(rename = "serverUuid")]
    pub server_uuid: String,
    #[serde(rename = "projectUuid")]
    pub project_uuid: String,
    #[serde(rename = "environmentName", default = "default_env_name")]
    pub environment_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordPressConfig {
    #[serde(rename = "dbUser")]
    pub db_user: String,
    #[serde(rename = "dbPassword")]
    pub db_password: String,
    #[serde(rename = "defaultAdminEmail")]
    pub default_admin_email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GloryConfig {
    #[serde(rename = "templateRepo")]
    pub template_repo: String,
    #[serde(rename = "libraryRepo")]
    pub library_repo: String,
    #[serde(rename = "defaultBranch", default = "default_branch")]
    pub default_branch: String,
}

fn default_env_name() -> String {
    "production".to_string()
}

fn default_branch() -> String {
    "main".to_string()
}
