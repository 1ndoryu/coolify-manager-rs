import type {
    RespuestaAuditoria,
    RespuestaBackups,
    RespuestaLogs,
    RespuestaMetricasDespliegue,
    RespuestaSalud,
    RespuestaSitios,
    RespuestaTargets,
    ResultadoOperacion,
} from "../tipos";

export const respuestaSitiosDemo: RespuestaSitios = {
    sites: [
        { name: "studio", domain: "https://nakomi.studio", target: "default", stack_uuid: "do8k4w8swccwwogoc0os0ck0", template: "Rust" },
        { name: "kamples", domain: "https://kamples.com", target: "default", stack_uuid: "mo4so4440c488g8woow4cow0", template: "WordPress" },
        { name: "glory-rest", domain: "http://restaurante.wandori.us", target: "default", stack_uuid: "b8s0cks444o0sogo8kg8wcgw", template: "Rust" },
        { name: "nakomi", domain: "https://task.nakomi.studio", target: "default", stack_uuid: "u00gc8ss4csc4cckkg4g00ks", template: "WordPress" },
        { name: "wandori", domain: "https://api.wandori.us", target: "default", stack_uuid: "csoc88c0gw8kc4cwcwosc48s", template: "Rust" },
        { name: "padel", domain: "https://materialdepadel.es", target: "default", stack_uuid: "zkcc040cc0scock4kcooowkc", template: "WordPress" },
        { name: "cap", domain: "https://cap.wandori.us", target: "default", stack_uuid: "qgskgw8wwc08o444o08wko8o", template: "WordPress" },
        { name: "guillermo", domain: "https://guillechatbots.es", target: "default", stack_uuid: "owck8sww4ogk8gskgwcsk4w0", template: "WordPress" },
    ],
    minecraft: [
        { name: "survival", memory: "2G", max_players: 20 },
    ],
};

const estadosDemo: Record<string, Partial<RespuestaSalud>> = {
    studio: { status_code: 200, healthy: true },
    kamples: { status_code: 200, healthy: true },
    "glory-rest": { status_code: 200, healthy: true },
    nakomi: { status_code: 200, healthy: true },
    wandori: { status_code: 200, healthy: true },
    padel: { status_code: 200, healthy: true },
    cap: { status_code: 503, healthy: false },
    guillermo: { status_code: 200, healthy: true },
};

export function obtenerSaludDemo(siteName: string): RespuestaSalud {
    const sitio = respuestaSitiosDemo.sites.find((item) => item.name === siteName);
    const estado = estadosDemo[siteName] ?? { status_code: null, healthy: false };

    return {
        site_name: siteName,
        url: sitio?.domain ?? "http://localhost",
        http_ok: estado.healthy ?? false,
        app_ok: estado.healthy ?? false,
        fatal_log_detected: false,
        status_code: estado.status_code ?? null,
        healthy: estado.healthy ?? false,
        details: estado.healthy ? ["Modo navegador: estado de muestra"] : ["Modo navegador: revisar health real con Tauri"],
    };
}

export function obtenerBackupsDemo(siteName: string): RespuestaBackups {
    return {
        site_name: siteName,
        backups: [
            { backup_id: `${siteName}-manual-20260510-1140`, tier: "manual", status: "Listo", created_at: "2026-05-10T11:40:00Z", label: "antes-rediseño-gui", artifact_count: 2 },
            { backup_id: `${siteName}-daily-20260510-0300`, tier: "daily", status: "Listo", created_at: "2026-05-10T03:00:00Z", label: null, artifact_count: 2 },
            { backup_id: `${siteName}-weekly-20260504-0300`, tier: "weekly", status: "Listo", created_at: "2026-05-04T03:00:00Z", label: null, artifact_count: 2 },
        ],
    };
}

export const respuestaTargetsDemo: RespuestaTargets = {
    default_target: "default",
    config_path: "config/settings.json",
    targets: [
        { name: "default", host: "66.94.100.241", user: "root", coolify_url: "https://coolify.nakomi.studio", site_count: 8 },
        { name: "vps2", host: "66.94.100.242", user: "root", coolify_url: "https://coolify-2.nakomi.studio", site_count: 0 },
    ],
};

export function obtenerAuditoriaDemo(target = "default"): RespuestaAuditoria {
    const esVps2 = target === "vps2";
    return {
        target,
        load_average: esVps2 ? "0.22 0.18 0.16" : "1.18 0.92 0.74",
        memory_summary: esVps2 ? "used=920MB free=6080MB total=7000MB" : "used=4380MB free=3620MB total=8000MB",
        disk_summary: esVps2 ? "used=18G available=82G use=18%" : "used=54G available=46G use=54%",
        docker_summary: "coolify=Up 2 days\ntraefik=Up 2 days\npostgres=Up 2 days",
        security_summary: "ufw=[active] fail2ban=[active]",
        recommendations: esVps2 ? [] : ["Revisar crecimiento de imágenes Docker semanalmente."],
        load_1m: esVps2 ? 0.22 : 1.18,
        load_5m: esVps2 ? 0.18 : 0.92,
        load_15m: esVps2 ? 0.16 : 0.74,
        memory_used_mb: esVps2 ? 920 : 4380,
        memory_free_mb: esVps2 ? 6080 : 3620,
        memory_total_mb: esVps2 ? 7000 : 8000,
        disk_use_percent: esVps2 ? 18 : 54,
    };
}

export function obtenerMetricasDemo(): RespuestaMetricasDespliegue {
    const generatedAt = new Date().toISOString();
    return {
        generated_at: generatedAt,
        metrics: respuestaSitiosDemo.sites.map((sitio, index) => {
            const cpu = sitio.name === "cap" ? 0 : Number((0.8 + index * 0.37).toFixed(2));
            const used = sitio.name === "cap" ? 0 : (220 + index * 38) * 1024 * 1024;
            const limit = sitio.name === "cap" ? 0 : 1024 * 1024 * 1024;
            return {
                site_name: sitio.name,
                target: sitio.target,
                status: sitio.name === "cap" ? "sin-contenedores" : "running",
                total_cpu_percent: cpu,
                memory_used_bytes: used,
                memory_limit_bytes: limit,
                memory_percent: limit > 0 ? (used / limit) * 100 : 0,
                containers: sitio.name === "cap" ? [] : [{
                    name: `${sitio.stack_uuid}_${sitio.template === "Rust" ? "app" : "wordpress"}`,
                    cpu_percent: cpu,
                    memory_usage: `${Math.round(used / 1024 / 1024)}MiB / 1GiB`,
                    memory_percent: limit > 0 ? (used / limit) * 100 : 0,
                    memory_used_bytes: used,
                    memory_limit_bytes: limit,
                }],
                updated_at: generatedAt,
            };
        }),
    };
}

export function obtenerLogsDemo(siteName: string): RespuestaLogs {
    return {
        site_name: siteName,
        container_target: "app",
        lines: 120,
        content: `[modo navegador] ${siteName}: abre la app con npm run dev para leer registros reales desde Tauri.`,
        stderr: "",
        exit_code: 0,
    };
}

export function obtenerOperacionDemo(siteName: string, accion: string): ResultadoOperacion {
    return {
        success: true,
        message: `[modo navegador] ${accion} preparado para ${siteName}`,
        details: "Las operaciones reales se ejecutan en la ventana Tauri iniciada con npm run dev.",
    };
}