/*
 * Sistema de configuracion.
 * Lee settings.json, expande variables de entorno y valida schema.
 * Compatible 1:1 con el formato del coolify-manager PowerShell.
 */

use crate::domain::{MinecraftServer, SiteConfig};
use crate::error::{ConfigError, CoolifyError};

use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

mod entidades;
pub use entidades::*;
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

impl Settings {
    /// Carga la configuracion desde settings.json con expansion de variables de entorno.
    pub fn load(config_path: &Path) -> std::result::Result<Self, CoolifyError> {
        if !config_path.exists() {
            return Err(ConfigError::FileNotFound {
                path: config_path.display().to_string(),
            }
            .into());
        }

        let raw =
            std::fs::read_to_string(config_path).map_err(|e| ConfigError::Parse(e.to_string()))?;
        let expanded = expand_env_vars(&raw);
        let settings: Settings =
            serde_json::from_str(&expanded).map_err(|e| ConfigError::Parse(e.to_string()))?;

        Ok(settings)
    }

    /// Carga con cache global (una sola lectura por proceso).
    pub fn load_cached(config_path: &Path) -> std::result::Result<&'static Settings, CoolifyError> {
        if let Some(cached) = CONFIG_CACHE.get() {
            return Ok(cached);
        }
        let settings = Self::load(config_path)?;
        /* get_or_init es atomico: si otro hilo inicializo primero (o si esta es la
         * primera inicializacion) siempre devuelve el valor vigente, sin `set` + `get`
         * consecutivos ni expect sobre un estado que dependia del orden de hilos. */
        Ok(CONFIG_CACHE.get_or_init(|| settings))
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

    pub fn get_target(
        &self,
        name: &str,
    ) -> std::result::Result<&DeploymentTargetConfig, CoolifyError> {
        self.targets
            .iter()
            .find(|target| target.name == name)
            .ok_or_else(|| {
                CoolifyError::Validation(format!("Destino '{name}' no encontrado en targets"))
            })
    }

    pub fn get_dns_provider(
        &self,
        name: &str,
    ) -> std::result::Result<&DnsProviderConfig, CoolifyError> {
        self.dns_providers
            .iter()
            .find(|provider| provider.name == name)
            .ok_or_else(|| {
                CoolifyError::Validation(format!(
                    "Proveedor DNS '{name}' no encontrado en dnsProviders"
                ))
            })
    }

    pub fn default_target(&self) -> DeploymentTargetConfig {
        DeploymentTargetConfig {
            name: "default".to_string(),
            vps: self.vps.clone(),
            coolify: self.coolify.clone(),
            maintenance_policy: None,
            security_policy: None,
            host_profile: None,
        }
    }

    pub fn resolve_site_target(
        &self,
        site: &SiteConfig,
    ) -> std::result::Result<DeploymentTargetConfig, CoolifyError> {
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
        /* [105A-7] Tauri dev ejecuta el binario desde CARGO_TARGET_DIR; la config vive en el repo. */
        Self::resolve_config_path_with_sources(
            explicit,
            std::env::var_os("COOLIFY_MANAGER_CONFIG").map(PathBuf::from),
            std::env::current_dir().ok(),
            std::env::current_exe().ok(),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        )
    }

    fn resolve_config_path_with_sources(
        explicit: Option<&Path>,
        env_override: Option<PathBuf>,
        current_dir: Option<PathBuf>,
        current_exe: Option<PathBuf>,
        manifest_dir: PathBuf,
    ) -> PathBuf {
        if let Some(path) = explicit {
            return path.to_path_buf();
        }

        if let Some(path) = env_override {
            return path;
        }

        let mut candidates = Vec::new();

        if let Some(dir) = current_dir.as_deref() {
            append_config_candidates(&mut candidates, dir);
        }

        append_config_candidates(&mut candidates, &manifest_dir);

        if let Some(exe_dir) = current_exe.as_deref().and_then(Path::parent) {
            append_config_candidates(&mut candidates, exe_dir);
        }

        candidates
            .iter()
            .find(|candidate| candidate.exists())
            .cloned()
            .unwrap_or_else(|| manifest_dir.join("config").join("settings.json"))
    }

    /// Agrega un sitio nuevo a la configuracion y persiste a disco.
    pub fn add_site(
        &mut self,
        site: SiteConfig,
        config_path: &Path,
    ) -> std::result::Result<(), CoolifyError> {
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
    pub fn update_site(
        &mut self,
        site: SiteConfig,
        config_path: &Path,
    ) -> std::result::Result<(), CoolifyError> {
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

    /// Elimina un sitio de la configuracion y persiste a disco.
    /// [119A-2] Solo se usa tras un borrado remoto verificado (`delete-site`):
    /// el sitio debe haber desaparecido de Coolify (404) antes de llamar aquí.
    pub fn remove_site(
        &mut self,
        site_name: &str,
        config_path: &Path,
    ) -> std::result::Result<(), CoolifyError> {
        let antes = self.sitios.len();
        self.sitios.retain(|s| s.nombre != site_name);
        if self.sitios.len() == antes {
            return Err(CoolifyError::Validation(format!(
                "Sitio '{site_name}' no encontrado para eliminar"
            )));
        }
        self.save(config_path)
    }

    /// Persiste la configuracion actual a disco.
    pub fn save(&self, config_path: &Path) -> std::result::Result<(), CoolifyError> {
        let json =
            serde_json::to_string_pretty(self).map_err(|e| ConfigError::Parse(e.to_string()))?;
        std::fs::write(config_path, json)?;
        Ok(())
    }
}

fn append_config_candidates(candidates: &mut Vec<PathBuf>, start_dir: &Path) {
    for ancestor in start_dir.ancestors() {
        let candidate = ancestor.join("config").join("settings.json");
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }
}

/// Regex del patron `${VAR_NAME}`.
/// El literal es constante y valido, asi que el motor no puede rechazarlo; si alguna
/// vez lo hiciera, `expand_env_vars` devuelve el texto sin expandir en vez de entrar
/// en panico durante la carga de configuracion.
fn env_var_regex() -> Option<&'static regex::Regex> {
    static RE: OnceLock<Option<regex::Regex>> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"\$\{([^}]+)\}").ok())
        .as_ref()
}

/// Expande patrones `${VAR_NAME}` con valores de variables de entorno.
fn expand_env_vars(input: &str) -> String {
    let Some(re) = env_var_regex() else {
        return input.to_string();
    };
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
    use tempfile::{NamedTempFile, TempDir};

    fn create_temp_config(json: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(json.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    fn write_config(root: &TempDir) -> PathBuf {
        let config_dir = root.path().join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("settings.json");
        std::fs::write(&config_path, "{}").unwrap();
        config_path
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
        assert!(matches!(
            err,
            CoolifyError::Config(ConfigError::FileNotFound { .. })
        ));
    }

    #[test]
    fn test_resolve_config_path_prefers_explicit_over_all_other_sources() {
        let explicit_root = TempDir::new().unwrap();
        let explicit_path = write_config(&explicit_root);

        let env_root = TempDir::new().unwrap();
        let env_path = write_config(&env_root);

        let current_root = TempDir::new().unwrap();
        let current_path = write_config(&current_root);

        let manifest_root = TempDir::new().unwrap();
        let manifest_path = write_config(&manifest_root);

        let resolved = Settings::resolve_config_path_with_sources(
            Some(&explicit_path),
            Some(env_path),
            Some(current_root.path().join("nested")),
            Some(current_root.path().join("bin").join("coolify-manager.exe")),
            manifest_root.path().to_path_buf(),
        );

        assert_eq!(resolved, explicit_path);
        assert_ne!(resolved, current_path);
        assert_ne!(resolved, manifest_path);
    }

    #[test]
    fn test_resolve_config_path_uses_current_dir_ancestors_for_gui_dev_runs() {
        let current_root = TempDir::new().unwrap();
        let current_path = write_config(&current_root);
        let nested_dir = current_root.path().join("gui").join("src-tauri");
        std::fs::create_dir_all(&nested_dir).unwrap();

        let manifest_root = TempDir::new().unwrap();

        let resolved = Settings::resolve_config_path_with_sources(
            None,
            None,
            Some(nested_dir),
            Some(PathBuf::from(
                r"C:\tmp\glory-target\debug\coolify-manager-gui.exe",
            )),
            manifest_root.path().to_path_buf(),
        );

        assert_eq!(resolved, current_path);
    }

    #[test]
    fn test_resolve_config_path_falls_back_to_manifest_dir_when_needed() {
        let manifest_root = TempDir::new().unwrap();
        let manifest_path = write_config(&manifest_root);

        let other_root = TempDir::new().unwrap();
        let unresolved_dir = other_root.path().join("sin-config");
        std::fs::create_dir_all(&unresolved_dir).unwrap();

        let resolved = Settings::resolve_config_path_with_sources(
            None,
            None,
            Some(unresolved_dir),
            Some(PathBuf::from(
                r"C:\tmp\glory-target\debug\coolify-manager-gui.exe",
            )),
            manifest_root.path().to_path_buf(),
        );

        assert_eq!(resolved, manifest_path);
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
            extra_domains: Vec::new(),
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
            repo_url: None,
            app_bin: crate::domain::default_app_bin(),
            frontend_dir: crate::domain::default_frontend_dir(),
            image_ref: None,
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
            extra_domains: Vec::new(),
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
            repo_url: None,
            app_bin: crate::domain::default_app_bin(),
            frontend_dir: crate::domain::default_frontend_dir(),
            image_ref: None,
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
        let default_target = settings
            .resolve_site_target(settings.get_site("a").unwrap())
            .unwrap();
        let named_target = settings
            .resolve_site_target(settings.get_site("b").unwrap())
            .unwrap();

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
