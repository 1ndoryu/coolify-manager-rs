use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WordPressSecurityReport {
    pub debug_enabled: bool,
    pub file_editor_disabled: bool,
    pub force_ssl_admin: bool,
    pub has_default_admin_username: bool,
    pub administrator_count: usize,
    pub recommendations: Vec<String>,
}

pub async fn audit_wordpress_security(
    ssh: &SshClient,
    container_id: &str,
) -> std::result::Result<WordPressSecurityReport, CoolifyError> {
    let config_probe = docker::docker_exec(
        ssh,
        container_id,
        "grep -E \"WP_DEBUG|DISALLOW_FILE_EDIT|FORCE_SSL_ADMIN\" /var/www/html/wp-config.php 2>/dev/null || true",
    )
    .await?;

    let users_script = r#"<?php
if (!isset($_SERVER['HTTP_HOST'])) { $_SERVER['HTTP_HOST'] = 'localhost'; }
define('ABSPATH', '/var/www/html/');
require_once ABSPATH . 'wp-load.php';
$admins = get_users(['role' => 'administrator', 'fields' => ['user_login']]);
$names = array_map(fn($user) => $user->user_login, $admins);
echo json_encode(['admins' => $names], JSON_UNESCAPED_SLASHES);
"#;
    let users_cmd = format!("echo '{}' > /tmp/cm_wp_audit.php && php /tmp/cm_wp_audit.php && rm -f /tmp/cm_wp_audit.php", users_script.replace('\'', "'\\''"));
    let users_output = docker::docker_exec(ssh, container_id, &users_cmd).await?;
    let admins_json: serde_json::Value = serde_json::from_str(users_output.stdout.trim()).unwrap_or_else(|_| serde_json::json!({"admins": []}));
    let admin_names: Vec<String> = admins_json
        .get("admins")
        .and_then(|value| value.as_array())
        .map(|items| items.iter().filter_map(|item| item.as_str().map(|value| value.to_string())).collect())
        .unwrap_or_default();

    let debug_enabled = config_probe.stdout.contains("WP_DEBUG', true") || config_probe.stdout.contains("WP_DEBUG',true");
    let file_editor_disabled = config_probe.stdout.contains("DISALLOW_FILE_EDIT") && config_probe.stdout.contains("true");
    let force_ssl_admin = config_probe.stdout.contains("FORCE_SSL_ADMIN") && config_probe.stdout.contains("true");
    let has_default_admin_username = admin_names.iter().any(|name| name.eq_ignore_ascii_case("admin"));

    let mut recommendations = Vec::new();
    if debug_enabled {
        recommendations.push("Desactivar WP_DEBUG en producción o limitarlo a ventanas de mantenimiento".to_string());
    }
    if !file_editor_disabled {
        recommendations.push("Añadir DISALLOW_FILE_EDIT=true para evitar edición desde wp-admin".to_string());
    }
    if !force_ssl_admin {
        recommendations.push("Añadir FORCE_SSL_ADMIN=true para endurecer acceso administrativo".to_string());
    }
    if has_default_admin_username {
        recommendations.push("Renombrar o retirar el usuario admin por ser objetivo trivial".to_string());
    }
    recommendations.push("La fuerza histórica de contraseñas no se puede inferir desde hashes; usar rotación desde el gestor".to_string());

    Ok(WordPressSecurityReport {
        debug_enabled,
        file_editor_disabled,
        force_ssl_admin,
        has_default_admin_username,
        administrator_count: admin_names.len(),
        recommendations,
    })
}

pub async fn rotate_admin_password(
    ssh: &SshClient,
    container_id: &str,
    username: &str,
    password: &str,
) -> std::result::Result<(), CoolifyError> {
    let php_script = format!(
        r#"<?php
if (!isset($_SERVER['HTTP_HOST'])) {{ $_SERVER['HTTP_HOST'] = 'localhost'; }}
define('ABSPATH', '/var/www/html/');
require_once ABSPATH . 'wp-load.php';
$user = get_user_by('login', '{username}');
if (!$user) {{
    fwrite(STDERR, 'Usuario admin no encontrado');
    exit(1);
}}
wp_set_password('{password}', $user->ID);
echo 'Password actualizada';
"#,
        username = username,
        password = password
    );

    let cmd = format!("echo '{}' > /tmp/cm_rotate_admin.php && php /tmp/cm_rotate_admin.php && rm -f /tmp/cm_rotate_admin.php", php_script.replace('\'', "'\\''"));
    let result = docker::docker_exec(ssh, container_id, &cmd).await?;
    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("{}\n{}", result.stdout, result.stderr),
        });
    }
    Ok(())
}