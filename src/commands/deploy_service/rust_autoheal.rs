use super::env_building::normalize_health_path;
use super::postgres_auth::base64_encode;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::services::{health_manager, volume_manager};

pub(crate) fn is_rust_network_probe_failure(report: &health_manager::HealthReport) -> bool {
    report
        .details
        .iter()
        .any(|detail| detail.contains("Rust network probe fallo"))
}

pub(crate) async fn recover_rust_network_probe_failure(
    ssh: &SshClient,
    service_dir: &str,
    compose_service: &str,
    site: &crate::domain::SiteConfig,
) -> std::result::Result<(), CoolifyError> {
    volume_manager::ensure_uploads_bind_mount(ssh, service_dir, &site.nombre, compose_service)
        .await?;
    let compose_image = ensure_compose_service_image_available(ssh, service_dir, compose_service)
        .await
        .map_err(|e| match e {
            CoolifyError::Validation(message) => CoolifyError::Validation(format!(
                "{message}\nRecovery sin build abortado: ejecuta deploy-service sin --skip-build para reconstruir antes de recrear {compose_service}."
            )),
            other => other,
        })?;
    tracing::info!("Recovery Rust network probe usara imagen existente: {compose_image}");

    let service_dir_quoted = shell_single_quote(service_dir);
    let compose_service_quoted = shell_single_quote(compose_service);
    let expected_bind = shell_single_quote(&format!("/data/uploads/{}:/app/uploads", site.nombre));
    let recover_cmd = format!(
        "cd {service_dir_quoted} || exit 2; \
         if ! grep -q -- {expected_bind} docker-compose.yml; then echo 'ABORT: uploads bind mount missing'; exit 2; fi; \
         docker compose up -d --no-build --force-recreate --no-deps {compose_service_quoted} 2>&1"
    );
    let result = ssh.execute(&recover_cmd).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "Recovery Rust network probe fallo: {}",
            command_output_summary(&result.stdout, &result.stderr)
        )));
    }

    volume_manager::verify_runtime_uploads_bind_mount(
        ssh,
        service_dir,
        compose_service,
        &site.nombre,
    )
    .await?;
    Ok(())
}

pub(crate) async fn install_rust_public_autoheal(
    ssh: &SshClient,
    site: &crate::domain::SiteConfig,
    stack_uuid: &str,
    service_dir: &str,
    compose_service: &str,
    public_health_url: &str,
) -> std::result::Result<(), CoolifyError> {
    let unit_name = format!("cm-autoheal-{}", systemd_safe_name(&site.nombre));
    let script_path = format!("/usr/local/bin/{unit_name}.sh");
    let service_path = format!("/etc/systemd/system/{unit_name}.service");
    let timer_path = format!("/etc/systemd/system/{unit_name}.timer");
    let health_path = normalize_health_path(&site.health_check.http_path);
    let expected_bind = format!("/data/uploads/{}:/app/uploads", site.nombre);

    let script = format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

SITE={site_name}
STACK_UUID={stack_uuid}
SERVICE_DIR={service_dir}
COMPOSE_SERVICE={compose_service}
PUBLIC_HEALTH_URL={public_health_url}
INTERNAL_HEALTH_PATH={internal_health_path}
EXPECTED_BIND={expected_bind}
LOG_TAG={log_tag}
COOLDOWN_FILE=/tmp/$LOG_TAG.last

log() {{
    logger -t "$LOG_TAG" "$*" || true
    printf '%s %s\n' "$LOG_TAG" "$*"
}}

public_ok() {{
    curl -fsS --max-time 10 "$PUBLIC_HEALTH_URL" >/dev/null
}}

ensure_proxy_network() {{
    if docker network inspect "$STACK_UUID" >/dev/null 2>&1; then
        docker network connect "$STACK_UUID" coolify-proxy >/dev/null 2>&1 || true
    fi
}}

ensure_app_coolify_network() {{
    cid=$(cd "$SERVICE_DIR" && docker compose ps -q "$COMPOSE_SERVICE" 2>/dev/null || true)
    if [ -n "$cid" ] && docker network inspect coolify >/dev/null 2>&1; then
        docker network connect coolify "$cid" >/dev/null 2>&1 || true
    fi
}}

if public_ok; then
    exit 0
fi

now=$(date +%s)
last=0
if [ -f "$COOLDOWN_FILE" ]; then
    last=$(cat "$COOLDOWN_FILE" 2>/dev/null || echo 0)
fi
if [ $((now - last)) -lt 120 ]; then
    log "public health failed but cooldown is active"
    exit 0
fi
printf '%s' "$now" > "$COOLDOWN_FILE"

log "public health failed; attempting proxy/app repair"
ensure_proxy_network
sleep 3
if public_ok; then
    log "recovered by reconnecting coolify-proxy to $STACK_UUID"
    exit 0
fi

container=$(cd "$SERVICE_DIR" && docker compose ps -q "$COMPOSE_SERVICE" 2>/dev/null || true)
if [ -n "$container" ]; then
    app_ip=$(docker inspect -f '{{{{with index .NetworkSettings.Networks "'"$STACK_UUID"'"}}}}{{{{.IPAddress}}}}{{{{end}}}}' "$container" 2>/dev/null || true)
    if [ -z "$app_ip" ]; then
        app_ip=$(docker inspect -f '{{{{range .NetworkSettings.Networks}}}}{{{{.IPAddress}}}} {{{{end}}}}{{{{end}}}}' "$container" 2>/dev/null | awk '{{print $1}}' || true)
    fi
    if [ -z "$app_ip" ] || ! curl -sf --max-time 5 "http://$app_ip:3000$INTERNAL_HEALTH_PATH" >/dev/null; then
        log "internal app probe failed; skipping no-build recreate"
        exit 1
    fi
else
    log "app container missing; trying no-build recreate"
fi

if ! grep -q -- "$EXPECTED_BIND" "$SERVICE_DIR/docker-compose.yml"; then
    log "abort: expected uploads bind mount missing from compose"
    exit 2
fi

image=$(cd "$SERVICE_DIR" && docker compose config --images 2>/dev/null | grep -v '^postgres:' | head -n 1)
if [ -z "$image" ] || ! docker image inspect "$image" >/dev/null 2>&1; then
    log "abort: compose image missing ($image)"
    exit 3
fi

cd "$SERVICE_DIR"
docker compose up -d --no-build --force-recreate --no-deps "$COMPOSE_SERVICE"
ensure_proxy_network
ensure_app_coolify_network
sleep 8
if public_ok; then
    log "public route recovered after no-build recreate"
    exit 0
fi

log "repair attempted but public health still fails"
exit 1
"#,
        site_name = shell_single_quote(&site.nombre),
        stack_uuid = shell_single_quote(stack_uuid),
        service_dir = shell_single_quote(service_dir),
        compose_service = shell_single_quote(compose_service),
        public_health_url = shell_single_quote(public_health_url),
        internal_health_path = shell_single_quote(&health_path),
        expected_bind = shell_single_quote(&expected_bind),
        log_tag = shell_single_quote(&unit_name),
    );

    let service = format!(
        r#"[Unit]
Description=Coolify Manager autoheal for {site_name}
After=docker.service network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart={script_path}
TimeoutStartSec=90
"#,
        site_name = site.nombre,
        script_path = script_path,
    );

    let timer = format!(
        r#"[Unit]
Description=Run Coolify Manager autoheal for {site_name}

[Timer]
OnBootSec=2min
OnUnitActiveSec=60s
AccuracySec=15s
Persistent=true
Unit={unit_name}.service

[Install]
WantedBy=timers.target
"#,
        site_name = site.nombre,
        unit_name = unit_name,
    );

    let script_b64 = base64_encode(script.as_bytes());
    let service_b64 = base64_encode(service.as_bytes());
    let timer_b64 = base64_encode(timer.as_bytes());
    let install_cmd = format!(
        "mkdir -p /usr/local/bin && \
                 echo {script_b64} | base64 -d > {script_path} && chmod 755 {script_path} && \
                 echo {service_b64} | base64 -d > {service_path} && \
                 echo {timer_b64} | base64 -d > {timer_path} && \
                 systemctl daemon-reload && systemctl enable --now {unit_name}.timer && \
                 systemctl is-active --quiet {unit_name}.timer && \
                 systemctl list-timers --no-pager --all {unit_name}.timer",
        script_path = shell_single_quote(&script_path),
        service_path = shell_single_quote(&service_path),
        timer_path = shell_single_quote(&timer_path),
        unit_name = shell_single_quote(&unit_name),
    );
    let result = ssh.execute(&install_cmd).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo instalar autoheal Rust: {}",
            command_output_summary(&result.stdout, &result.stderr)
        )));
    }
    if !result.stdout.trim().is_empty() {
        println!("      {}", result.stdout.trim().replace('\n', "\n      "));
    }

    println!("      Autoheal instalado: {unit_name}.timer (cada 60s)");
    Ok(())
}

pub(crate) async fn ensure_compose_service_image_available(
    ssh: &SshClient,
    service_dir: &str,
    compose_service: &str,
) -> std::result::Result<String, CoolifyError> {
    let service_dir_quoted = shell_single_quote(service_dir);
    let compose_service_quoted = shell_single_quote(compose_service);
    let image_check_cmd = format!(
        "cd {service_dir_quoted} || exit 2; \
         svc={compose_service_quoted}; \
            image=$(docker compose config 2>/dev/null | sed -n \"/^  ${{svc}}:/,/^  [A-Za-z0-9_.-]\\+:/p\" | awk '$1 == \"image:\" {{ print $2; exit }}'); \
            if [ -z \"$image\" ]; then image=$(docker compose config --images 2>/dev/null | grep -E \"\\-${{svc}}$\" | head -n 1); fi; \
            if [ -z \"$image\" ]; then image=$(docker compose config --images 2>/dev/null | grep -v '^postgres:' | grep -v 'busybox' | head -n 1); fi; \
            if [ -z \"$image\" ]; then echo 'No se pudo detectar la imagen del servicio en docker compose config'; exit 3; fi; \
            echo \"$image\"; \
            docker image inspect \"$image\" >/dev/null 2>&1"
    );
    let result = ssh.execute(&image_check_cmd).await?;
    let image = result.stdout.lines().next().unwrap_or_default().trim();
    if result.success() && !image.is_empty() {
        return Ok(image.to_string());
    }

    let output = command_output_summary(&result.stdout, &result.stderr);
    let image_context = if image.is_empty() {
        "imagen desconocida".to_string()
    } else {
        format!("imagen detectada '{image}'")
    };
    Err(CoolifyError::Validation(format!(
        "La {image_context} no existe localmente; abortando antes de recrear {compose_service}. {output}"
    )))
}

pub(crate) fn command_output_summary(stdout: &str, stderr: &str) -> String {
    let stdout = stdout.trim();
    let stderr = stderr.trim();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => "sin salida de Docker".to_string(),
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (false, false) => format!("stdout:\n{stdout}\nstderr:\n{stderr}"),
    }
}

pub(crate) fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) fn systemd_safe_name(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = sanitized.trim_matches('-');
    if trimmed.is_empty() {
        "site".to_string()
    } else {
        trimmed.to_string()
    }
}
