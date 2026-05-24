use crate::config::{DeploymentTargetConfig, FirewallSecurityPolicyConfig, VpsConfig};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use serde::Serialize;
use std::collections::BTreeSet;

const UFW_BACKUP_PATH: &str = "/root/coolify-manager-ufw-backup.tar.gz";
const FAIL2BAN_JAIL_PATH: &str = "/etc/fail2ban/jail.d/99-coolify-manager.conf";
const FAIL2BAN_JAIL_BACKUP_PATH: &str = "/etc/fail2ban/jail.d/99-coolify-manager.conf.bak";

#[derive(Debug, Clone)]
pub struct EnforceHostSecurityRequest {
    pub dry_run: bool,
    pub apply: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnforceHostSecurityReport {
    pub target: String,
    pub applied: bool,
    pub reconnect_validated: bool,
    pub current_client_ip: String,
    pub ufw_backup_path: String,
    pub fail2ban_jail_path: String,
    pub firewall_summary: String,
    pub fail2ban_summary: String,
    pub applied_steps: Vec<String>,
    pub warnings: Vec<String>,
}

pub async fn enforce_target(
    target: &DeploymentTargetConfig,
    request: &EnforceHostSecurityRequest,
) -> std::result::Result<EnforceHostSecurityReport, CoolifyError> {
    let policy = target.security_policy.as_ref().ok_or_else(|| {
        CoolifyError::Validation(format!(
            "Target '{}' sin securityPolicy; no hay politica host-level que aplicar",
            target.name
        ))
    })?;
    let firewall_policy = policy.firewall.as_ref().ok_or_else(|| {
        CoolifyError::Validation(format!(
            "Target '{}' sin securityPolicy.firewall; no hay politica de firewall que aplicar",
            target.name
        ))
    })?;
    if !firewall_policy.enabled {
        return Err(CoolifyError::Validation(format!(
            "Target '{}' tiene securityPolicy.firewall.enabled=false; no corresponde aplicar enforcement",
            target.name
        )));
    }

    let ssh_key = target.vps.ssh_key.as_deref().ok_or_else(|| {
        CoolifyError::Validation(format!(
            "Target '{}' no tiene sshKey configurada; endurecer firewall sin clave validada seria inseguro",
            target.name
        ))
    })?;

    let allowed_tcp_ports = normalize_ports(firewall_policy);
    let trusted_source_ips = policy
        .ssh
        .as_ref()
        .map(|ssh| ssh.trusted_source_ips.clone())
        .unwrap_or_default();
    let jail_content = render_fail2ban_jail(&trusted_source_ips);

    let mut warnings = Vec::new();
    if trusted_source_ips.is_empty() {
        warnings.push(
            "securityPolicy.ssh.trustedSourceIps esta vacio; el puerto 22 quedara abierto globalmente en UFW."
                .to_string(),
        );
    }
    if policy.ssh.is_none() {
        warnings.push(
            "No hay securityPolicy.ssh; solo se aplicara firewall/fail2ban segun puertos declarados."
                .to_string(),
        );
    }
    warnings.push(format!("sshKey validada para manager: {ssh_key}"));

    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    let current_client_ip = exec_trim(
        &ssh,
        "bash -lc 'client_ip=$(printf %s \"${SSH_CONNECTION%% *}\"); printf %s \"${client_ip:-unknown}\"'",
    )
    .await?;
    if !trusted_source_ips.is_empty() && !trusted_source_ips.iter().any(|ip| ip == &current_client_ip) {
        return Err(CoolifyError::Validation(format!(
            "La IP cliente actual '{}' no esta en trustedSourceIps ({}); abortando para no cortar SSH del manager",
            current_client_ip,
            trusted_source_ips.join(", ")
        )));
    }

    let mut applied_steps = build_preview_steps(&allowed_tcp_ports, &trusted_source_ips);
    applied_steps.push("fail2ban dejara activo el jail sshd con backend systemd e ignoreip segun la politica SSH declarada.".to_string());

    if request.dry_run || !request.apply {
        return Ok(EnforceHostSecurityReport {
            target: target.name.clone(),
            applied: false,
            reconnect_validated: false,
            current_client_ip,
            ufw_backup_path: UFW_BACKUP_PATH.to_string(),
            fail2ban_jail_path: FAIL2BAN_JAIL_PATH.to_string(),
            firewall_summary: "preview".to_string(),
            fail2ban_summary: "preview".to_string(),
            applied_steps,
            warnings,
        });
    }

    let ufw_was_active = exec_trim(
        &ssh,
        "bash -lc 'if command -v ufw >/dev/null 2>&1 && ufw status 2>/dev/null | head -n 1 | grep -qi \"Status: active\"; then echo yes; else echo no; fi'",
    )
    .await?;
    let fail2ban_was_active = exec_trim(
        &ssh,
        "bash -lc 'if systemctl is-active fail2ban >/dev/null 2>&1; then echo yes; else echo no; fi'",
    )
    .await?;

    let apply_script = format!(
        "set -e\nexport DEBIAN_FRONTEND=noninteractive\nlock_wait=0\nwhile true; do\n    lock_pid=\"\"\n    if command -v fuser >/dev/null 2>&1; then\n        lock_pid=$(fuser /var/lib/dpkg/lock-frontend 2>/dev/null | awk 'NR==1 {{print $1}}')\n    fi\n    if [ -z \"$lock_pid\" ]; then\n        lock_pid=$(pgrep -x apt-get 2>/dev/null | head -1 || true)\n    fi\n    if [ -z \"$lock_pid\" ]; then\n        break\n    fi\n    if [ \"$lock_wait\" -ge 900 ]; then\n        cmd=$(tr '\\0' ' ' < /proc/$lock_pid/cmdline 2>/dev/null || echo unknown)\n        echo FIREWALL_APT_LOCK_TIMEOUT pid=$lock_pid cmd=$cmd waited=${{lock_wait}}s\n        exit 99\n    fi\n    sleep 15\n    lock_wait=$((lock_wait + 15))\ndone\nif dpkg --audit 2>/dev/null | grep -q .; then\n    dpkg --configure -a\nfi\napt-get update\napt-get install -y ufw fail2ban\nmkdir -p /root /etc/fail2ban/jail.d\ntar -czf {ufw_backup_path} /etc/ufw\nif [ -f {fail2ban_jail_path} ]; then cp -a {fail2ban_jail_path} {fail2ban_jail_backup_path}; else rm -f {fail2ban_jail_backup_path}; fi\nprintf %s {jail_content} > {fail2ban_jail_path}\nchmod 644 {fail2ban_jail_path}\nufw --force reset\nufw default deny incoming\nufw default allow outgoing\n{ufw_rules}\nufw --force enable\nsystemctl enable fail2ban >/dev/null 2>&1 || true\nsystemctl restart fail2ban\nsystemctl is-active fail2ban >/dev/null\necho HOST_SECURITY_APPLIED",
        ufw_backup_path = shell_single_quote(UFW_BACKUP_PATH),
        fail2ban_jail_path = shell_single_quote(FAIL2BAN_JAIL_PATH),
        fail2ban_jail_backup_path = shell_single_quote(FAIL2BAN_JAIL_BACKUP_PATH),
        jail_content = shell_single_quote(&jail_content),
        ufw_rules = render_ufw_rules(&allowed_tcp_ports, &trusted_source_ips),
    );
    let apply_result = ssh
        .execute(&format!("bash -lc {}", shell_single_quote(&apply_script)))
        .await?;
    if !apply_result.success() || !apply_result.stdout.contains("HOST_SECURITY_APPLIED") {
        return Err(CoolifyError::Validation(format!(
            "No se pudo aplicar enforcement host-level: {}{}",
            apply_result.stdout, apply_result.stderr
        )));
    }
    applied_steps.push("UFW y fail2ban aplicados segun la politica declarada del target.".to_string());

    let mut validation_client = SshClient::from_vps(&VpsConfig {
        ip: target.vps.ip.clone(),
        user: target.vps.user.clone(),
        ssh_key: target.vps.ssh_key.clone(),
        ssh_password: None,
    });
    let reconnect_validated = validation_client.connect().await.is_ok();
    if !reconnect_validated {
        let rollback_script = format!(
            "set -e\nif [ -f {ufw_backup_path} ]; then tar -xzf {ufw_backup_path} -C /; fi\nif [ -f {fail2ban_jail_backup_path} ]; then mv -f {fail2ban_jail_backup_path} {fail2ban_jail_path}; else rm -f {fail2ban_jail_path}; fi\nif [ {ufw_was_active} = yes ]; then\n    ufw --force disable || true\n    ufw --force enable\nelse\n    ufw --force disable || true\nfi\nif [ {fail2ban_was_active} = yes ]; then\n    systemctl restart fail2ban || true\nelse\n    systemctl stop fail2ban || true\nfi\necho HOST_SECURITY_ROLLED_BACK",
            ufw_backup_path = shell_single_quote(UFW_BACKUP_PATH),
            fail2ban_jail_path = shell_single_quote(FAIL2BAN_JAIL_PATH),
            fail2ban_jail_backup_path = shell_single_quote(FAIL2BAN_JAIL_BACKUP_PATH),
            ufw_was_active = ufw_was_active,
            fail2ban_was_active = fail2ban_was_active,
        );
        let rollback_result = ssh
            .execute(&format!("bash -lc {}", shell_single_quote(&rollback_script)))
            .await?;
        if !rollback_result.success() || !rollback_result.stdout.contains("HOST_SECURITY_ROLLED_BACK") {
            return Err(CoolifyError::RolledBack(
                "La reconexion SSH fallo y el rollback de firewall/fail2ban tambien fallo; hace falta revisar el host manualmente."
                    .to_string(),
            ));
        }
        return Err(CoolifyError::RolledBack(
            "La reconexion SSH fallo tras aplicar firewall/fail2ban; se revirtio automaticamente la politica host-level."
                .to_string(),
        ));
    }

    let firewall_summary = exec_trim(
        &validation_client,
        r#"bash -lc 'ufw status numbered 2>/dev/null | tr "\n" ";"'"#,
    )
    .await?;
    let fail2ban_summary = exec_trim(
        &validation_client,
        r#"bash -lc 'state=$(systemctl is-active fail2ban 2>/dev/null || echo inactive); sshd=$(fail2ban-client status sshd 2>/dev/null | tr "\n" ";" || true); printf "state=%s sshd=%s" "$state" "${sshd:-unknown}"'"#,
    )
    .await?;

    applied_steps.push("Reconexión SSH validada tras activar UFW/fail2ban.".to_string());
    Ok(EnforceHostSecurityReport {
        target: target.name.clone(),
        applied: true,
        reconnect_validated,
        current_client_ip,
        ufw_backup_path: UFW_BACKUP_PATH.to_string(),
        fail2ban_jail_path: FAIL2BAN_JAIL_PATH.to_string(),
        firewall_summary: empty_as_unknown(&firewall_summary),
        fail2ban_summary: empty_as_unknown(&fail2ban_summary),
        applied_steps,
        warnings,
    })
}

fn normalize_ports(firewall_policy: &FirewallSecurityPolicyConfig) -> Vec<u16> {
    let mut ports = firewall_policy
        .allowed_tcp_ports
        .iter()
        .copied()
        .filter(|port| *port != 22)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    ports.sort_unstable();
    ports
}

fn build_preview_steps(allowed_tcp_ports: &[u16], trusted_source_ips: &[String]) -> Vec<String> {
    let mut steps = Vec::new();
    steps.push("Instalar/asegurar paquetes ufw y fail2ban en el host remoto.".to_string());
    if trusted_source_ips.is_empty() {
        steps.push("UFW permitira tcp/22 desde cualquier origen porque no hay trustedSourceIps declaradas.".to_string());
    } else {
        steps.push(format!(
            "UFW permitira tcp/22 solo desde: {}.",
            trusted_source_ips.join(", ")
        ));
    }
    if allowed_tcp_ports.is_empty() {
        steps.push("La politica no declara puertos TCP adicionales aparte de SSH.".to_string());
    } else {
        steps.push(format!(
            "UFW permitira puertos TCP publicos: {}.",
            allowed_tcp_ports
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    steps.push("UFW quedara con default deny incoming y default allow outgoing.".to_string());
    steps
}

fn render_ufw_rules(allowed_tcp_ports: &[u16], trusted_source_ips: &[String]) -> String {
    let mut lines = Vec::new();
    if trusted_source_ips.is_empty() {
        lines.push("ufw allow 22/tcp".to_string());
    } else {
        for ip in trusted_source_ips {
            lines.push(format!("ufw allow from {} to any port 22 proto tcp", shell_single_quote(ip)));
        }
    }
    for port in allowed_tcp_ports {
        lines.push(format!("ufw allow {}/tcp", port));
    }
    lines.join("\n")
}

fn render_fail2ban_jail(trusted_source_ips: &[String]) -> String {
    let mut ignore_ips = vec!["127.0.0.1/8".to_string(), "::1".to_string()];
    ignore_ips.extend(trusted_source_ips.iter().cloned());
    format!(
        "[DEFAULT]\nignoreip = {}\nbantime = 1h\nfindtime = 10m\nmaxretry = 5\n\n[sshd]\nenabled = true\nbackend = systemd\nport = 22\n\n",
        ignore_ips.join(" ")
    )
}

async fn exec_trim(ssh: &SshClient, command: &str) -> std::result::Result<String, CoolifyError> {
    let result = ssh.execute(command).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "Fallo ejecutando comando remoto: {}{}",
            result.stdout, result.stderr
        )));
    }
    Ok(result.stdout.trim().to_string())
}

fn empty_as_unknown(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}