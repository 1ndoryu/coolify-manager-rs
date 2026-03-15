/*
 * Sistema de configuracion.
 * Lee settings.json, expande variables de entorno y valida schema.
 * Compatible 1:1 con el formato del coolify-manager PowerShell.
 */

use crate::domain::{MinecraftServer, SiteConfig, SmtpConfig};
use crate::error::{ConfigError, CoolifyError};

use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static CONFIG_CACHE: OnceLock<Settings> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub vps: VpsConfig,
    pub coolify: CoolifyConfig,
    pub wordpress: WordPressConfig,
    pub glory: GloryConfig,
    #[serde(rename = "backupStorage", default)]
    pub backup_storage: BackupStorageConfig,
    #[serde(rename = "dnsProviders", default)]
    pub dns_providers: Vec<DnsProviderConfig>,
    /* SMTP global — se usa en todos los sitios que no tengan smtpConfig propio */
    #[serde(default)]
    pub smtp: Option<SmtpGlobalConfig>,
    #[serde(default)]
    pub targets: Vec<DeploymentTargetConfig>,
    #[serde(default)]
    pub sitios: Vec<SiteConfig>,
    #[serde(default)]
    pub minecraft: Vec<MinecraftServer>,
}

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

fn default_smtp_port() -> u16 { 587 }
fn default_smtp_from_name() -> String { "WordPress".to_string() }
fn default_smtp_secure() -> String { "tls".to_string() }
fn default_backup_local_dir() -> String { "backups".to_string() }
fn default_contabo_api_base_url() -> String { "https://api.contabo.com".to_string() }
fn default_contabo_auth_base_url() -> String { "https://auth.contabo.com/auth/realms/contabo/protocol/openid-connect/token".to_string() }

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

impl Settings {
    /// Carga la configuracion desde settings.json con expansion de variables de entorno.
    pub fn load(config_path: &Path) -> std::result::Result<Self, CoolifyError> {
        if !config_path.exists() {
            return Err(ConfigError::FileNotFound {
                path: config_path.display().to_string(),
            }
            .into());
        }

        let raw = std::fs::read_to_string(config_path).map_err(|e| ConfigError::Parse(e.to_string()))?;
        let expanded = expand_env_vars(&raw);
        let settings: Settings = serde_json::from_str(&expanded).map_err(|e| ConfigError::Parse(e.to_string()))?;

        Ok(settings)
    }

    /// Carga con cache global (una sola lectura por proceso).
    pub fn load_cached(config_path: &Path) -> std::result::Result<&'static Settings, CoolifyError> {
        if let Some(cached) = CONFIG_CACHE.get() {
            return Ok(cached);
        }
        let settings = Self::load(config_path)?;
        /* Si otro hilo inicializo primero, usamos su version */
        let _ = CONFIG_CACHE.set(settings);
        Ok(CONFIG_CACHE.get().expect("CONFIG_CACHE recien inicializado"))
    }

    /// Busca un sitio por nombre.
    pub fn get_site(&self, name: &str) -> std::result::Result<&SiteConfig, CoolifyError> {
        self.sitios
            .iter()
            .find(|s| s.nombre == name)
            .ok_or_else(|| CoolifyError::SiteNotFound(name.to_string()))
    }

    /// Busca un servidor Minecraft por nombre.
    pub fn get_minecraft(&self, name: &str) -> std::result::Result<&MinecraftServer, CoolifyError> {
        self.minecraft
            .iter()
            .find(|m| m.server_name == name)
            .ok_or_else(|| CoolifyError::SiteNotFound(format!("minecraft:{name}")))
    }

    pub fn get_target(&self, name: &str) -> std::result::Result<&DeploymentTargetConfig, CoolifyError> {
        self.targets
            .iter()
            .find(|target| target.name == name)
            .ok_or_else(|| CoolifyError::Validation(format!("Destino '{name}' no encontrado en targets")))
    }

    pub fn get_dns_provider(&self, name: &str) -> std::result::Result<&DnsProviderConfig, CoolifyError> {
        self.dns_providers
            .iter()
            .find(|provider| provider.name == name)
            .ok_or_else(|| CoolifyError::Validation(format!("Proveedor DNS '{name}' no encontrado en dnsProviders")))
    }

    pub fn default_target(&self) -> DeploymentTargetConfig {
        DeploymentTargetConfig {
            name: "default".to_string(),
            vps: self.vps.clone(),
            coolify: self.coolify.clone(),
        }
    }

    pub fn resolve_site_target(&self, site: &SiteConfig) -> std::result::Result<DeploymentTargetConfig, CoolifyError> {
        match site.target.as_deref() {
            Some(name) => Ok(self.get_target(name)?.clone()),
            None => Ok(self.default_target()),
        }
    }

    /// Obtiene el password de DB de forma segura (env var > config).
    pub fn get_db_password(&self, site_name: &str) -> SecretString {
        let env_key = format!("DB_PASSWORD_{}", site_name.to_uppercase().replace('-', "_"));
        if let Ok(val) = std::env::var(&env_key) {
            return SecretString::from(val);
        }
        if let Ok(val) = std::env::var("COOLIFY_DB_PASSWORD") {
            return SecretString::from(val);
        }
        SecretString::from(self.wordpress.db_password.clone())
    }

    /// Resuelve la ruta al archivo de configuracion.
    pub fn resolve_config_path(explicit: Option<&Path>) -> PathBuf {
        if let Some(p) = explicit {
            return p.to_path_buf();
        }
        /* Buscar relativo al ejecutable primero */
        if let Ok(exe) = std::env::current_exe() {
            let candidate = exe.parent().unwrap_or(Path::new(".")).join("config").join("settings.json");
            if candidate.exists() {
                return candidate;
            }
        }
        /* Fallback: directorio actual */
        PathBuf::from("config").join("settings.json")
    }

    /// Agrega un sitio nuevo a la configuracion y persiste a disco.
    pub fn add_site(&mut self, site: SiteConfig, config_path: &Path) -> std::result::Result<(), CoolifyError> {
        if self.sitios.iter().any(|s| s.nombre == site.nombre) {
            return Err(CoolifyError::Validation(format!(
                "Sitio '{}' ya existe en configuracion",
                site.nombre
            )));
        }
        self.sitios.push(site);
        self.save(config_path)
    }

    /// Actualiza un sitio existente (placeholder con stackUuid vacio) y persiste a disco.
    pub fn update_site(&mut self, site: SiteConfig, config_path: &Path) -> std::result::Result<(), CoolifyError> {
        if let Some(existing) = self.sitios.iter_mut().find(|s| s.nombre == site.nombre) {
            *existing = site;
        } else {
            return Err(CoolifyError::Validation(format!(
                "Sitio '{}' no encontrado para actualizar",
                site.nombre
            )));
        }
        self.save(config_path)
    }

    /// Persiste la configuracion actual a disco.
    pub fn save(&self, config_path: &Path) -> std::result::Result<(), CoolifyError> {
        let json = serde_json::to_string_pretty(self).map_err(|e| ConfigError::Parse(e.to_string()))?;
        std::fs::write(config_path, json)?;
        Ok(())
    }
}

/// Expande patrones `${VAR_NAME}` con valores de variables de entorno.
fn expand_env_vars(input: &str) -> String {
    let re = regex::Regex::new(r"\$\{([^}]+)\}").expect("regex valido");
    re.replace_all(input, |caps: &regex::Captures| {
        let var_name = &caps[1];
        std::env::var(var_name).unwrap_or_else(|_| {
            tracing::warn!("Variable de entorno '{var_name}' no definida, dejando vacio");
            String::new()
        })
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_config(json: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(json.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn test_load_minimal_config() {
        let json = r#"{
            "vps": { "ip": "1.2.3.4", "user": "root" },
            "coolify": {
                "baseUrl": "http://1.2.3.4:8000",
                "apiToken": "test-token",
                "serverUuid": "srv-1",
                "projectUuid": "proj-1"
            },
            "wordpress": {
                "dbUser": "manager",
                "dbPassword": "secret",
                "defaultAdminEmail": "a@b.com"
            },
            "glory": {
                "templateRepo": "https://github.com/test/template.git",
                "libraryRepo": "https://github.com/test/lib.git"
            }
        }"#;

        let f = create_temp_config(json);
        let settings = Settings::load(f.path()).unwrap();

        assert_eq!(settings.vps.ip, "1.2.3.4");
        assert_eq!(settings.vps.user, "root");
        assert_eq!(settings.coolify.api_token, "test-token");
        assert_eq!(settings.wordpress.db_user, "manager");
        assert_eq!(settings.glory.default_branch, "main");
        assert_eq!(settings.backup_storage.local_dir, "backups");
        assert!(settings.backup_storage.remote.is_none());
        assert!(settings.dns_providers.is_empty());
        assert!(settings.sitios.is_empty());
        assert!(settings.minecraft.is_empty());
    }

    #[test]
    fn test_load_with_sites() {
        let json = r#"{
            "vps": { "ip": "1.2.3.4", "user": "root" },
            "coolify": {
                "baseUrl": "http://1.2.3.4:8000",
                "apiToken": "tok",
                "serverUuid": "s",
                "projectUuid": "p"
            },
            "wordpress": { "dbUser": "u", "dbPassword": "p", "defaultAdminEmail": "a@b.c" },
            "glory": { "templateRepo": "r1", "libraryRepo": "r2" },
            "sitios": [
                { "nombre": "blog", "dominio": "https://blog.com", "stackUuid": "abc123" },
                { "nombre": "shop", "dominio": "https://shop.com" }
            ]
        }"#;

        let f = create_temp_config(json);
        let settings = Settings::load(f.path()).unwrap();

        assert_eq!(settings.sitios.len(), 2);
        assert_eq!(settings.get_site("blog").unwrap().nombre, "blog");
        assert_eq!(
            settings.get_site("blog").unwrap().stack_uuid.as_deref(),
            Some("abc123")
        );
        assert!(settings.get_site("nonexistent").is_err());
    }

    #[test]
    fn test_env_var_expansion() {
        std::env::set_var("TEST_CM_TOKEN", "expanded-token");
        let input = r#"{"token": "${TEST_CM_TOKEN}", "other": "literal"}"#;
        let result = expand_env_vars(input);
        assert!(result.contains("expanded-token"));
        assert!(result.contains("literal"));
        std::env::remove_var("TEST_CM_TOKEN");
    }

    #[test]
    fn test_env_var_missing_leaves_empty() {
        std::env::remove_var("NONEXISTENT_VAR_CM_TEST");
        let input = r#"{"val": "${NONEXISTENT_VAR_CM_TEST}"}"#;
        let result = expand_env_vars(input);
        assert!(result.contains(r#""val": """#));
    }

    #[test]
    fn test_config_file_not_found() {
        let result = Settings::load(Path::new("/nonexistent/path/settings.json"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, CoolifyError::Config(ConfigError::FileNotFound { .. })));
    }

    #[test]
    fn test_add_site() {
        let json = r#"{
            "vps": { "ip": "1.2.3.4", "user": "root" },
            "coolify": { "baseUrl": "u", "apiToken": "t", "serverUuid": "s", "projectUuid": "p" },
            "wordpress": { "dbUser": "u", "dbPassword": "p", "defaultAdminEmail": "a@b.c" },
            "glory": { "templateRepo": "r1", "libraryRepo": "r2" },
            "sitios": []
        }"#;

        let f = create_temp_config(json);
        let mut settings = Settings::load(f.path()).unwrap();

        let new_site = SiteConfig {
            nombre: "nuevo".to_string(),
            dominio: "https://nuevo.com".to_string(),
            target: None,
            stack_uuid: Some("uuid-123".to_string()),
            glory_branch: "main".to_string(),
            library_branch: "main".to_string(),
            theme_name: "glory".to_string(),
            skip_react: false,
            template: crate::domain::StackTemplate::Wordpress,
            php_config: None,
            smtp_config: None,
            disable_wp_cron: false,
            backup_policy: crate::domain::BackupPolicy::default(),
            health_check: crate::domain::HealthCheckConfig::default(),
            dns_config: None,
        };

        settings.add_site(new_site, f.path()).unwrap();
        assert_eq!(settings.sitios.len(), 1);
        assert_eq!(settings.sitios[0].nombre, "nuevo");

        /* Verificar que se persistio */
        let reloaded = Settings::load(f.path()).unwrap();
        assert_eq!(reloaded.sitios.len(), 1);
        assert_eq!(reloaded.sitios[0].nombre, "nuevo");
    }

    #[test]
    fn test_add_duplicate_site_fails() {
        let json = r#"{
            "vps": { "ip": "1.2.3.4", "user": "root" },
            "coolify": { "baseUrl": "u", "apiToken": "t", "serverUuid": "s", "projectUuid": "p" },
            "wordpress": { "dbUser": "u", "dbPassword": "p", "defaultAdminEmail": "a@b.c" },
            "glory": { "templateRepo": "r1", "libraryRepo": "r2" },
            "sitios": [{ "nombre": "blog", "dominio": "https://blog.com" }]
        }"#;

        let f = create_temp_config(json);
        let mut settings = Settings::load(f.path()).unwrap();

        let dup = SiteConfig {
            nombre: "blog".to_string(),
            dominio: "https://blog2.com".to_string(),
            target: None,
            stack_uuid: None,
            glory_branch: "main".to_string(),
            library_branch: "main".to_string(),
            theme_name: "glory".to_string(),
            skip_react: false,
            template: crate::domain::StackTemplate::Wordpress,
            php_config: None,
            smtp_config: None,
            disable_wp_cron: false,
            backup_policy: crate::domain::BackupPolicy::default(),
            dns_config: None,
            health_check: crate::domain::HealthCheckConfig::default(),
        };

        assert!(settings.add_site(dup, f.path()).is_err());
    }

    #[test]
    fn test_resolve_site_target_default_and_named() {
        let json = r#"{
            "vps": { "ip": "1.2.3.4", "user": "root" },
            "coolify": { "baseUrl": "http://1.2.3.4:8000", "apiToken": "tok", "serverUuid": "srv-a", "projectUuid": "proj-a" },
            "wordpress": { "dbUser": "u", "dbPassword": "p", "defaultAdminEmail": "a@b.c" },
            "glory": { "templateRepo": "r1", "libraryRepo": "r2" },
            "targets": [
                {
                    "name": "vps2",
                    "vps": { "ip": "5.6.7.8", "user": "root", "sshPassword": "abc" },
                    "coolify": { "baseUrl": "http://5.6.7.8:8000", "apiToken": "tok-b", "serverUuid": "srv-b", "projectUuid": "proj-b" }
                }
            ],
            "sitios": [
                { "nombre": "a", "dominio": "https://a.com" },
                { "nombre": "b", "dominio": "https://b.com", "target": "vps2" }
            ]
        }"#;

        let f = create_temp_config(json);
        let settings = Settings::load(f.path()).unwrap();
        let default_target = settings.resolve_site_target(settings.get_site("a").unwrap()).unwrap();
        let named_target = settings.resolve_site_target(settings.get_site("b").unwrap()).unwrap();

        assert_eq!(default_target.vps.ip, "1.2.3.4");
        assert_eq!(named_target.vps.ip, "5.6.7.8");
    }

    #[test]
    fn test_get_db_password_priority() {
        let json = r#"{
            "vps": { "ip": "1.2.3.4", "user": "root" },
            "coolify": { "baseUrl": "u", "apiToken": "t", "serverUuid": "s", "projectUuid": "p" },
            "wordpress": { "dbUser": "u", "dbPassword": "config-pass", "defaultAdminEmail": "a@b.c" },
            "glory": { "templateRepo": "r1", "libraryRepo": "r2" }
        }"#;

        let f = create_temp_config(json);
        let settings = Settings::load(f.path()).unwrap();

        /* Sin env vars, usa config */
        std::env::remove_var("DB_PASSWORD_BLOG");
        std::env::remove_var("COOLIFY_DB_PASSWORD");
        let pass = settings.get_db_password("blog");
        assert_eq!(pass.expose_secret(), "config-pass");

        /* Con env var especifica del sitio */
        std::env::set_var("DB_PASSWORD_BLOG", "site-specific");
        let pass = settings.get_db_password("blog");
        assert_eq!(pass.expose_secret(), "site-specific");
        std::env::remove_var("DB_PASSWORD_BLOG");
    }
}
