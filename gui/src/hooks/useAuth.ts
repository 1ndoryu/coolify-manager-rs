/*
 * [125A-3] Hook de autenticación para la consola operativa.
 * Token en memoria (no localStorage): se pierde al cerrar la pestaña — intencional para seguridad.
 * LOCAL_MODE: si el backend reporta local_mode=true, se autentica sin token.
 * Gotcha: al refrescar la página, el usuario necesita volver a hacer login (excepto LOCAL_MODE).
 */

import { useState, useEffect, useCallback } from "react";
import { apiLocalUrl, setAuthToken } from "../servicios/clienteCoolify";

interface EstadoAuth {
    autenticado: boolean;
    email: string | null;
    token: string | null;
    localMode: boolean;
    cargando: boolean;
    error: string | null;
}

export interface HookAuth extends EstadoAuth {
    login: (email: string, password: string) => Promise<boolean>;
    logout: () => Promise<void>;
}

/* Token privado en módulo — compartido entre hook y clienteCoolify sin localStorage */
let tokenEnMemoria: string | null = null;

function getTokenMemoria(): string | null {
    return tokenEnMemoria;
}

function setTokenMemoria(t: string | null): void {
    tokenEnMemoria = t;
    setAuthToken(t);
}

export function useAuth(): HookAuth {
    const [estado, setEstado] = useState<EstadoAuth>({
        autenticado: false,
        email: null,
        token: null,
        localMode: false,
        cargando: true,
        error: null,
    });

    const verificarSesion = useCallback(async () => {
        const headers: Record<string, string> = { "content-type": "application/json" };
        const t = getTokenMemoria();
        if (t) headers["authorization"] = `Bearer ${t}`;

        try {
            const resp = await fetch(`${apiLocalUrl()}/api/auth/me`, { headers });
            if (resp.ok) {
                const data = await resp.json() as { email: string; local_mode: boolean };
                setEstado({
                    autenticado: true,
                    email: data.email,
                    token: t,
                    localMode: data.local_mode,
                    cargando: false,
                    error: null,
                });
            } else {
                setTokenMemoria(null);
                setEstado(s => ({ ...s, autenticado: false, email: null, token: null, cargando: false }));
            }
        } catch {
            setTokenMemoria(null);
            setEstado(s => ({ ...s, autenticado: false, email: null, token: null, cargando: false }));
        }
    }, []);

    useEffect(() => {
        void verificarSesion();
    }, [verificarSesion]);

    const login = useCallback(async (email: string, password: string): Promise<boolean> => {
        setEstado(s => ({ ...s, cargando: true, error: null }));
        try {
            const resp = await fetch(`${apiLocalUrl()}/api/auth/login`, {
                method: "POST",
                headers: { "content-type": "application/json" },
                body: JSON.stringify({ email, password }),
            });
            if (resp.ok) {
                const data = await resp.json() as { token: string; email: string };
                setTokenMemoria(data.token);
                setEstado({
                    autenticado: true,
                    email: data.email,
                    token: data.token,
                    localMode: false,
                    cargando: false,
                    error: null,
                });
                return true;
            } else {
                const err = await resp.json() as { error?: string };
                setEstado(s => ({
                    ...s,
                    cargando: false,
                    error: err.error ?? "Error desconocido",
                }));
                return false;
            }
        } catch {
            setEstado(s => ({
                ...s,
                cargando: false,
                error: "No se pudo conectar al servidor",
            }));
            return false;
        }
    }, []);

    const logout = useCallback(async () => {
        try {
            const headers: Record<string, string> = { "content-type": "application/json" };
            const t = getTokenMemoria();
            if (t) headers["authorization"] = `Bearer ${t}`;
            await fetch(`${apiLocalUrl()}/api/auth/logout`, { method: "POST", headers });
        } catch {
            /* ignorar errores de logout: el token local se limpia de todas formas */
        }
        setTokenMemoria(null);
        setEstado({
            autenticado: false,
            email: null,
            token: null,
            localMode: false,
            cargando: false,
            error: null,
        });
    }, []);

    return { ...estado, login, logout };
}
