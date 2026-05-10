import { invoke } from "@tauri-apps/api/core";
import {
    obtenerAuditoriaDemo,
    obtenerBackupsDemo,
    obtenerLogsDemo,
    obtenerMetricasDemo,
    obtenerOperacionDemo,
    obtenerSaludDemo,
    respuestaSitiosDemo,
    respuestaTargetsDemo,
} from "../datos/demoCoolify";
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

export type ModoCliente = "tauri" | "navegador";

export interface ResultadoCliente<T> {
    datos: T;
    modo: ModoCliente;
}

type ComandoGui =
    | "list_sites"
    | "list_targets"
    | "health_check"
    | "list_backups"
    | "audit_vps"
    | "deployment_metrics"
    | "view_logs"
    | "manual_backup"
    | "restart_site"
    | "redeploy_site"
    | "get_config_path";

function runtimeTauriDisponible(): boolean {
    if (typeof window === "undefined") {
        return false;
    }

    const runtime = (window as Window & {
        __TAURI_INTERNALS__?: { invoke?: unknown };
    }).__TAURI_INTERNALS__;

    return typeof runtime?.invoke === "function";
}

function esperarDemo<T>(datos: T): Promise<T> {
    return new Promise((resolve) => {
        window.setTimeout(() => resolve(datos), 120);
    });
}

function obtenerDemo<T>(comando: ComandoGui, args: Record<string, unknown>): Promise<T> {
    switch (comando) {
        case "list_sites":
            return esperarDemo(respuestaSitiosDemo as T);
        case "list_targets":
            return esperarDemo(respuestaTargetsDemo as T);
        case "health_check":
            return esperarDemo(obtenerSaludDemo(String(args.siteName ?? "studio")) as T);
        case "list_backups":
            return esperarDemo(obtenerBackupsDemo(String(args.siteName ?? "studio")) as T);
        case "audit_vps":
            return esperarDemo(obtenerAuditoriaDemo(String(args.target ?? "default")) as T);
        case "deployment_metrics":
            return esperarDemo(obtenerMetricasDemo() as T);
        case "view_logs":
            return esperarDemo(obtenerLogsDemo(String(args.siteName ?? "studio")) as T);
        case "manual_backup":
            return esperarDemo(obtenerOperacionDemo(String(args.siteName ?? "studio"), "Copia manual") as T);
        case "restart_site":
            return esperarDemo(obtenerOperacionDemo(String(args.siteName ?? "studio"), "Reinicio") as T);
        case "redeploy_site":
            return esperarDemo(obtenerOperacionDemo(String(args.siteName ?? "studio"), "Redespliegue protegido") as T);
        case "get_config_path":
            return esperarDemo("config/settings.json" as T);
    }
}

export async function ejecutarComandoGui<T>(
    comando: ComandoGui,
    args: Record<string, unknown> = {},
): Promise<ResultadoCliente<T>> {
    /* [105A-10] El navegador debe ser usable sin Tauri; Tauri queda como backend real cuando existe. */
    if (runtimeTauriDisponible()) {
        return { datos: await invoke<T>(comando, args), modo: "tauri" };
    }

    return { datos: await obtenerDemo<T>(comando, args), modo: "navegador" };
}

export type RespuestasGui =
    | RespuestaSitios
    | RespuestaSalud
    | RespuestaBackups
    | RespuestaTargets
    | RespuestaAuditoria
    | RespuestaMetricasDespliegue
    | RespuestaLogs
    | ResultadoOperacion
    | string;