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

export function useComandoTauri<T>(comando: string): EstadoComando<T> {
    const [datos, setDatos] = useState<T | null>(null);
    const [cargando, setCargando] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const ejecutar = useCallback(async (...args: unknown[]): Promise<T | null> => {
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
