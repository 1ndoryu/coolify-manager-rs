/* Split 119A-5 de lightweight_runtime_manager.rs — plantillas y scripts remotos.
 * Codigo verbatim del original; solo cambia visibilidad a pub(super). */

use super::tipos::RestoreOutputMetadata;

use base64::Engine;
use rand::distributions::Alphanumeric;
use rand::Rng;

pub(super) fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) fn base64_encode(value: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(value)
}

pub(super) fn generate_access_user(site_name: &str) -> String {
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

pub(super) fn generate_access_password() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(24)
        .map(char::from)
        .collect()
}

pub(super) fn build_static_compose(http_port: u16) -> String {
    format!(
        "services:\n  web:\n    image: nginx:alpine\n    restart: unless-stopped\n    ports:\n      - '127.0.0.1:{http_port}:80'\n    volumes:\n      - ./public:/usr/share/nginx/html:ro\n"
    )
}

pub(super) fn build_caddyfile(fqdn: &str, http_port: u16) -> String {
    format!(
        "{fqdn} {{\n    encode zstd gzip\n    reverse_proxy 127.0.0.1:{http_port}\n    header {{\n        X-Content-Type-Options nosniff\n        X-Frame-Options SAMEORIGIN\n        Referrer-Policy strict-origin-when-cross-origin\n    }}\n}}\n"
    )
}

pub(super) fn build_sshd_match(site_name: &str, access_user: &str) -> String {
    format!(
        "Match User {access_user}\n    ChrootDirectory /srv/hosting/{site_name}\n    ForceCommand internal-sftp -d /public\n    PasswordAuthentication yes\n    AllowTcpForwarding no\n    X11Forwarding no\n"
    )
}

pub(super) fn build_site_metadata(access_user: &str, http_port: u16, fqdn: &str) -> String {
    format!(
        "LIGHT_SITE_USER={access_user}\nLIGHT_SITE_HTTP_PORT={http_port}\nLIGHT_SITE_FQDN={fqdn}\n"
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_provision_static_script(
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

pub(super) fn build_restore_script(
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

pub(super) fn parse_restore_output(raw: &str) -> RestoreOutputMetadata {
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

pub(super) fn build_default_index(site_name: &str, fqdn: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"es\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n  <title>{site_name}</title>\n  <style>body{{font-family:system-ui,sans-serif;margin:0;padding:3rem;background:#f6f4ef;color:#1f2937}}main{{max-width:48rem;margin:0 auto}}h1{{font-size:2rem;margin-bottom:0.5rem}}p{{line-height:1.6}}</style>\n</head>\n<body>\n  <main>\n    <h1>{site_name}</h1>\n    <p>Tu hosting lightweight ya está operativo en {fqdn}.</p>\n    <p>Sube tus archivos por SFTP al directorio <strong>/public</strong>.</p>\n  </main>\n</body>\n</html>\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
