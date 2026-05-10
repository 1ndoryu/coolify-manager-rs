/*
 * Hook para invocar comandos Tauri con estado de carga y error.
 */

import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

interface EstadoComando<T> {
    datos: T | null;
    cargando: boolean;
    error: string | null;
    ejecutar: (...args: unknown[]) => Promise<T | null>;
}

export const MENSAJE_TAURI_REQUERIDO =
    "La interfaz necesita el runtime nativo de Tauri para ejecutar comandos.";

function tauriRuntimeDisponible(): boolean {
    /* [105A-6] La GUI puede abrirse por Vite durante debug; sin runtime Tauri, invoke rompe la pantalla. */
    if (typeof window === "undefined") {
        return false;
    }

    const runtime = (window as Window & {
        __TAURI_INTERNALS__?: { invoke?: unknown };
    }).__TAURI_INTERNALS__;

    return typeof runtime?.invoke === "function";
}

export function useComandoTauri<T>(comando: string): EstadoComando<T> {
    const [datos, setDatos] = useState<T | null>(null);
    const [cargando, setCargando] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const ejecutar = useCallback(async (...args: unknown[]): Promise<T | null> => {
        if (!tauriRuntimeDisponible()) {
            setError(MENSAJE_TAURI_REQUERIDO);
            setCargando(false);
            return null;
        }

        setCargando(true);
        setError(null);
        try {
            const argObj = args[0] && typeof args[0] === "object" ? args[0] : {};
            const resultado = await invoke<T>(comando, argObj as Record<string, unknown>);
            setDatos(resultado);
            return resultado;
        } catch (err) {
            const mensaje = err instanceof Error ? err.message : String(err);
            setError(mensaje);
            return null;
        } finally {
            setCargando(false);
        }
    }, [comando]);

    return { datos, cargando, error, ejecutar };
}
