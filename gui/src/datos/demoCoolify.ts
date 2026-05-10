import type { RespuestaBackups, RespuestaSalud, RespuestaSitios } from "../tipos";

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
        details: estado.healthy ? ["Browser preview: estado demo"] : ["Browser preview: revisar health real desde Tauri"],
    };
}

export function obtenerBackupsDemo(siteName: string): RespuestaBackups {
    return {
        site_name: siteName,
        backups: [
            { backup_id: `${siteName}-manual-20260510-1140`, tier: "manual", status: "Ready", created_at: "2026-05-10T11:40:00Z", label: "antes-rediseño-gui", artifact_count: 2 },
            { backup_id: `${siteName}-daily-20260510-0300`, tier: "daily", status: "Ready", created_at: "2026-05-10T03:00:00Z", label: null, artifact_count: 2 },
            { backup_id: `${siteName}-weekly-20260504-0300`, tier: "weekly", status: "Ready", created_at: "2026-05-04T03:00:00Z", label: null, artifact_count: 2 },
        ],
    };
}