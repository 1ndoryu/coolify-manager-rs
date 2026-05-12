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
    RespuestaBackupsGlobal,
    RespuestaLogs,
    RespuestaMetricasDespliegue,
    RespuestaSalud,
    RespuestaSitios,
    RespuestaTargets,
    ResultadoOperacion,
} from "../tipos";

export type ModoCliente = "tauri" | "local" | "demo";

export interface ResultadoCliente<T> {
    datos: T;
    modo: ModoCliente;
}

export function etiquetaModoCliente(modo: ModoCliente): string {
    if (modo === "tauri") return "Modo real Tauri";
    if (modo === "local") return "Modo real local";
    return "Modo demo";
}

export function claseModoCliente(modo: ModoCliente): string {
    return modo === "demo" ? "badgeAdvertencia" : "badgeExito";
}

type ComandoGui =
    | "list_sites"
    | "list_targets"
    | "health_check"
    | "list_backups"
    | "list_all_backups"
    | "audit_vps"
    | "deployment_metrics"
    | "create_site"
    | "view_logs"
    | "manual_backup"
    | "restart_site"
    | "redeploy_site"
    | "get_config_path";

interface EntradaCacheGui {
    expiraEn: number;
    resultado: ResultadoCliente<unknown>;
}

const cacheLecturasGui = new Map<string, EntradaCacheGui>();
const lecturasEnCursoGui = new Map<string, Promise<ResultadoCliente<unknown>>>();

/* [105A-28] Cache cliente para Tauri/local/demo: evita repetir lecturas lentas al cambiar de vista.
 * Gotcha: las operaciones write limpian cache y los botones manuales usan force=true. */

function ttlComando(comando: ComandoGui): number | null {
    switch (comando) {
        case "list_sites":
        case "list_targets":
            return 60_000;
        case "health_check":
        case "audit_vps":
            return 20_000;
        case "deployment_metrics":
            return 12_000;
        case "list_backups":
            return 180_000;
        case "list_all_backups":
        case "get_config_path":
            return 300_000;
        default:
            return null;
    }
}

function claveCache(comando: ComandoGui, args: Record<string, unknown>): string {
    const normalizados = Object.entries(args)
        .filter(([clave]) => clave !== "force")
        .sort(([a], [b]) => a.localeCompare(b));

    return `${comando}:${JSON.stringify(Object.fromEntries(normalizados))}`;
}

function esRefrescoForzado(args: Record<string, unknown>): boolean {
    return args.force === true;
}

function limpiarCacheTrasOperacion(comando: ComandoGui) {
    if (["create_site", "manual_backup", "restart_site", "redeploy_site"].includes(comando)) {
        cacheLecturasGui.clear();
        lecturasEnCursoGui.clear();
    }
}

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

export function apiLocalUrl(): string {
    return String(import.meta.env.VITE_COOLIFY_MANAGER_API_URL ?? "http://127.0.0.1:8787").replace(/\/$/, "");
}

/* [125A-3] Token en memoria — no persiste en localStorage para evitar XSS.
 * useAuth.ts llama setAuthToken tras login/logout exitoso. */
let tokenAuth: string | null = null;

export function setAuthToken(token: string | null): void {
    tokenAuth = token;
}

function demoHabilitado(): boolean {
    return import.meta.env.VITE_COOLIFY_MANAGER_DEMO === "1";
}

async function ejecutarApiLocal<T>(comando: ComandoGui, args: Record<string, unknown>): Promise<T> {
    const headers: Record<string, string> = { "content-type": "application/json" };
    if (tokenAuth) headers["authorization"] = `Bearer ${tokenAuth}`;
    const respuesta = await fetch(`${apiLocalUrl()}/api/command`, {
        method: "POST",
        headers,
        body: JSON.stringify({ command: comando, args }),
    });

    if (!respuesta.ok) {
        let detalle = respuesta.statusText;
        try {
            const cuerpo = await respuesta.json() as { error?: string };
            detalle = cuerpo.error ?? detalle;
        } catch {
            detalle = await respuesta.text();
        }
        throw new Error(detalle || `API local respondió HTTP ${respuesta.status}`);
    }

    return await respuesta.json() as T;
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
        case "list_all_backups":
            return esperarDemo({
                backups: respuestaSitiosDemo.sites.flatMap((sitio) => obtenerBackupsDemo(sitio.name).backups.map((backup) => ({
                    ...backup,
                    site_name: sitio.name,
                    domain: sitio.domain,
                    target: sitio.target,
                    template: sitio.template,
                }))),
                errors: [],
            } as T);
        case "audit_vps":
            return esperarDemo(obtenerAuditoriaDemo(String(args.target ?? "default")) as T);
        case "deployment_metrics":
            return esperarDemo(obtenerMetricasDemo() as T);
        case "create_site":
            return esperarDemo(obtenerOperacionDemo(String(args.name ?? "nuevo-sitio"), "Creacion de sitio") as T);
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

async function ejecutarComandoGuiSinCache<T>(
    comando: ComandoGui,
    args: Record<string, unknown> = {},
): Promise<ResultadoCliente<T>> {
    /* [105A-24] El navegador usa API local real; demo solo queda con VITE_COOLIFY_MANAGER_DEMO=1. */
    if (runtimeTauriDisponible()) {
        return { datos: await invoke<T>(comando, args), modo: "tauri" };
    }

    try {
        return { datos: await ejecutarApiLocal<T>(comando, args), modo: "local" };
    } catch (error) {
        if (demoHabilitado()) {
            return { datos: await obtenerDemo<T>(comando, args), modo: "demo" };
        }

        throw new Error(
            `API local de coolify-manager no disponible en ${apiLocalUrl()}. Ejecuta npm run dev:web desde coolify-manager-rs. Detalle: ${error instanceof Error ? error.message : String(error)}`,
        );
    }
}

export async function ejecutarComandoGui<T>(
    comando: ComandoGui,
    args: Record<string, unknown> = {},
): Promise<ResultadoCliente<T>> {
    const ttl = ttlComando(comando);
    const clave = ttl !== null ? claveCache(comando, args) : null;
    const usaCache = clave !== null && !esRefrescoForzado(args);

    if (usaCache) {
        const cached = cacheLecturasGui.get(clave);
        if (cached && cached.expiraEn > Date.now()) {
            return cached.resultado as ResultadoCliente<T>;
        }

        const enCurso = lecturasEnCursoGui.get(clave);
        if (enCurso) {
            return (await enCurso) as ResultadoCliente<T>;
        }
    }

    const promesa = ejecutarComandoGuiSinCache<T>(comando, args);
    if (!clave || ttl === null) {
        const resultado = await promesa;
        limpiarCacheTrasOperacion(comando);
        return resultado;
    }

    lecturasEnCursoGui.set(clave, promesa as Promise<ResultadoCliente<unknown>>);
    try {
        const resultado = await promesa;
        cacheLecturasGui.set(clave, {
            expiraEn: Date.now() + ttl,
            resultado: resultado as ResultadoCliente<unknown>,
        });
        return resultado;
    } finally {
        lecturasEnCursoGui.delete(clave);
    }
}

export type RespuestasGui =
    | RespuestaSitios
    | RespuestaSalud
    | RespuestaBackups
    | RespuestaBackupsGlobal
    | RespuestaTargets
    | RespuestaAuditoria
    | RespuestaMetricasDespliegue
    | RespuestaLogs
    | ResultadoOperacion
    | string;