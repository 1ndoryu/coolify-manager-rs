/* Split 119A-5 de host_optimization_manager.rs — aplicación de ajustes host.
 * Código verbatim del original; solo cambia visibilidad a pub(super). */

use super::diagnostico::{docker_live_restore_enabled, sh_quote, swap_is_active, thp_is_disabled};
use super::tipos::{
    HostOptimizationRequest, HostSnapshot, DOCKER_DAEMON_CONFIG_PATH, SWAPFILE_PATH,
    SYSCTL_CONFIG_PATH, THP_SERVICE_PATH,
};
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

pub(super) async fn ensure_swap(
    ssh: &SshClient,
    swap_gb: u16,
) -> std::result::Result<String, CoolifyError> {
    let command = format!("sh -lc {}", sh_quote(&build_swap_script(swap_gb)));
    let result = ssh.execute(&command).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo asegurar swap en el host: {}{}",
            result.stdout, result.stderr
        )));
    }

    let details = result
        .stdout
        .trim()
        .lines()
        .last()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| "swap activa".to_string());
    Ok(format!("Swap asegurada: {details}"))
}

pub(super) async fn ensure_sysctl_profile(
    ssh: &SshClient,
    swappiness: u8,
    vfs_cache_pressure: u16,
    overcommit_memory: u8,
) -> std::result::Result<String, CoolifyError> {
    let content = format!(
        "vm.swappiness={}\nvm.vfs_cache_pressure={}\nvm.overcommit_memory={}\n",
        swappiness, vfs_cache_pressure, overcommit_memory
    );
    let script = format!(
        "set -e\nmkdir -p /etc/sysctl.d\nprintf %s {} > {}\nif ! sysctl --system >/dev/null 2>&1; then sysctl -p {} >/dev/null 2>&1; fi\necho SYSCTL_READY",
        sh_quote(&content), SYSCTL_CONFIG_PATH, SYSCTL_CONFIG_PATH
    );
    let command = format!("sh -lc {}", sh_quote(&script));
    let result = ssh.execute(&command).await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo aplicar sysctl host-level: {}{}",
            result.stdout, result.stderr
        )));
    }
    Ok(format!(
        "Sysctl persistido en {} (vm.swappiness={}, vm.vfs_cache_pressure={}, vm.overcommit_memory={}).",
        SYSCTL_CONFIG_PATH, swappiness, vfs_cache_pressure, overcommit_memory
    ))
}

pub(super) async fn ensure_thp_disabled(
    ssh: &SshClient,
) -> std::result::Result<String, CoolifyError> {
    let service_content = "[Unit]\nDescription=Disable Transparent Huge Pages\nAfter=local-fs.target\n\n[Service]\nType=oneshot\nExecStart=/bin/sh -c 'echo never > /sys/kernel/mm/transparent_hugepage/enabled; echo never > /sys/kernel/mm/transparent_hugepage/defrag'\n\n[Install]\nWantedBy=multi-user.target\n";
    let script = format!(
        "set -e\nif [ -f /sys/kernel/mm/transparent_hugepage/enabled ]; then echo never > /sys/kernel/mm/transparent_hugepage/enabled; fi\nif [ -f /sys/kernel/mm/transparent_hugepage/defrag ]; then echo never > /sys/kernel/mm/transparent_hugepage/defrag; fi\nprintf %s {} > {}\nchmod 644 {}\nsystemctl daemon-reload\nsystemctl enable --now cm-disable-thp.service >/dev/null 2>&1\necho THP_READY",
        sh_quote(service_content), THP_SERVICE_PATH, THP_SERVICE_PATH
    );
    let result = ssh
        .execute(&format!("bash -lc {}", sh_quote(&script)))
        .await?;
    if !result.success() || !result.stdout.contains("THP_READY") {
        return Err(CoolifyError::Validation(format!(
            "No se pudo desactivar THP: {}{}",
            result.stdout, result.stderr
        )));
    }
    Ok(format!(
        "THP desactivado en runtime y persistido via {}.",
        THP_SERVICE_PATH
    ))
}

pub(super) async fn ensure_docker_live_restore(
    ssh: &SshClient,
) -> std::result::Result<String, CoolifyError> {
    let script = format!(
        "set -e\nmkdir -p /etc/docker\npython3 -c {}\nif systemctl reload docker >/dev/null 2>&1; then echo DOCKER_RELOADED; else echo DOCKER_RELOAD_PENDING; fi",
        sh_quote("import json, pathlib; p=pathlib.Path('/etc/docker/daemon.json'); data=json.loads(p.read_text()) if p.exists() and p.stat().st_size else {}; data['live-restore']=True; p.write_text(json.dumps(data, indent=2)+'\\n')")
    );
    let result = ssh
        .execute(&format!("bash -lc {}", sh_quote(&script)))
        .await?;
    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo persistir live-restore en Docker: {}{}",
            result.stdout, result.stderr
        )));
    }
    let outcome = if result.stdout.contains("DOCKER_RELOADED") {
        "config persistida y daemon recargado"
    } else {
        "config persistida; Docker requerira restart/reboot controlado para activarse"
    };
    Ok(format!(
        "Docker live-restore en {}: {}.",
        DOCKER_DAEMON_CONFIG_PATH, outcome
    ))
}

pub(super) fn build_swap_script(swap_gb: u16) -> String {
    let swap_mb = u32::from(swap_gb) * 1024;
    let required_free_mb = swap_mb + 1024;

    format!(
        "set -e\nfree_mb=$(df -Pm / | awk 'NR==2 {{print $4}}')\nif swapon --show --noheadings | grep -q .; then\n  swapon --show --noheadings --output NAME,SIZE,USED,PRIO\n  exit 0\nfi\nif [ \"${{free_mb:-0}}\" -lt \"{required_free_mb}\" ]; then\n  echo INSUFFICIENT_DISK free_mb=${{free_mb:-0}} required_mb={required_free_mb}\n  exit 1\nfi\nif [ -f {SWAPFILE_PATH} ]; then\n  swapoff {SWAPFILE_PATH} 2>/dev/null || true\n  rm -f {SWAPFILE_PATH}\nfi\nif command -v fallocate >/dev/null 2>&1; then\n  fallocate -l {swap_mb}M {SWAPFILE_PATH} || dd if=/dev/zero of={SWAPFILE_PATH} bs=1M count={swap_mb} status=none\nelse\n  dd if=/dev/zero of={SWAPFILE_PATH} bs=1M count={swap_mb} status=none\nfi\nchmod 600 {SWAPFILE_PATH}\nmkswap {SWAPFILE_PATH} >/dev/null\nswapon {SWAPFILE_PATH}\ngrep -qE '^[^#]*\\s{SWAPFILE_PATH}\\s+none\\s+swap\\s+' /etc/fstab || printf '%s\\n' '{SWAPFILE_PATH} none swap sw 0 0' >> /etc/fstab\nswapon --show --noheadings --output NAME,SIZE,USED,PRIO"
    )
}

pub(super) fn build_recommendations(
    before: &HostSnapshot,
    after: &HostSnapshot,
    request: &HostOptimizationRequest,
) -> Vec<String> {
    let mut recommendations = Vec::new();
    let load_1m = after
        .load_average
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(0.0);

    if !swap_is_active(&after.swap_summary) {
        recommendations.push(format!(
            "El host sigue sin swap activa. Si no es un dry-run, revisar permisos o espacio y volver a ejecutar con --swap-gb {}.",
            request.swap_gb
        ));
    } else if !swap_is_active(&before.swap_summary) {
        recommendations.push(
            "La swap ya quedo activa; sirve como colchon para picos de memoria, pero no sustituye una correccion de CPU/I/O.".to_string(),
        );
    }

    if load_1m > after.cpu_count as f32 {
        recommendations.push(format!(
            "El load ({load_1m:.2}) sigue por encima de los {} vCPU; toca revisar workers/colas o mover carga fuera del host.",
            after.cpu_count
        ));
    }

    if after.pressure_summary.contains("io_full[avg10=")
        && !after.pressure_summary.contains("io_full[avg10=0.00")
    {
        recommendations.push(
            "Sigue habiendo presion de I/O en el host; optimizar WordPress no va a eliminar por si solo una latencia de disco del nodo.".to_string(),
        );
    }

    if after.top_processes.contains("php")
        || after.top_processes.contains("redis")
        || after.top_processes.contains("node")
    {
        recommendations.push(
            "Usa la foto de procesos calientes para bajar concurrencia de workers/scheduler o separar workloads ruidosos del mismo VPS.".to_string(),
        );
    }

    if request.disable_thp && !thp_is_disabled(&after.thp_summary) {
        recommendations.push(
            "THP no quedo realmente en never; revisar permisos de sysfs o el servicio cm-disable-thp.service.".to_string(),
        );
    }

    if request.docker_live_restore && !docker_live_restore_enabled(&after.docker_runtime_summary) {
        recommendations.push(
            "live-restore quedo persistido solo a nivel de config o sigue inactivo; conviene validarlo tras el siguiente restart controlado de Docker o reboot del host.".to_string(),
        );
    }

    if recommendations.is_empty() {
        recommendations.push(
            "Host estable tras el ajuste base; repetir medicion de latencia de la app y solo luego decidir si hace falta migrar carga.".to_string(),
        );
    }

    recommendations
}
