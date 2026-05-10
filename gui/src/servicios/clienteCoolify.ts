import { invoke } from "@tauri-apps/api/core";
import { obtenerBackupsDemo, obtenerSaludDemo, respuestaSitiosDemo } from "../datos/demoCoolify";
import type { RespuestaBackups, RespuestaSalud, RespuestaSitios } from "../tipos";

export type ModoCliente = "tauri" | "navegador";

export interface ResultadoCliente<T> {
    datos: T;
    modo: ModoCliente;
}

type ComandoGui = "list_sites" | "health_check" | "list_backups";

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
        case "health_check":
            return esperarDemo(obtenerSaludDemo(String(args.siteName ?? "studio")) as T);
        case "list_backups":
            return esperarDemo(obtenerBackupsDemo(String(args.siteName ?? "studio")) as T);
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

export type RespuestasGui = RespuestaSitios | RespuestaSalud | RespuestaBackups;