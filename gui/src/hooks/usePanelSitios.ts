import { useCallback, useEffect, useMemo, useState } from "react";
import { ejecutarComandoGui, type ModoCliente } from "../servicios/clienteCoolify";
import type { RespuestaBackups, RespuestaSalud, RespuestaSitios, SitioResumen } from "../tipos";

export type EstadoOperativo = "online" | "offline" | "checking" | "unknown";

export interface EstadoSitio {
    estado: EstadoOperativo;
    statusCode: number | null;
    actualizado: string | null;
    detalle: string;
}

const estadoInicial: EstadoSitio = {
    estado: "unknown",
    statusCode: null,
    actualizado: null,
    detalle: "Sin verificar",
};

function estadoDesdeSalud(salud: RespuestaSalud): EstadoSitio {
    return {
        estado: salud.healthy ? "online" : "offline",
        statusCode: salud.status_code,
        actualizado: new Date().toISOString(),
        detalle: salud.details[0] ?? (salud.healthy ? "Operativo" : "Revisar"),
    };
}

export function usePanelSitios() {
    const [sitios, setSitios] = useState<SitioResumen[]>([]);
    const [modoCliente, setModoCliente] = useState<ModoCliente>("navegador");
    const [cargandoSitios, setCargandoSitios] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [busqueda, setBusqueda] = useState("");
    const [estados, setEstados] = useState<Record<string, EstadoSitio>>({});
    const [sitioBackupsActivo, setSitioBackupsActivo] = useState<string | null>(null);
    const [backups, setBackups] = useState<RespuestaBackups | null>(null);
    const [cargandoBackups, setCargandoBackups] = useState(false);

    const refrescarEstadoSitio = useCallback(async (siteName: string) => {
        setEstados((actual) => ({
            ...actual,
            [siteName]: { ...(actual[siteName] ?? estadoInicial), estado: "checking" },
        }));

        try {
            const resultado = await ejecutarComandoGui<RespuestaSalud>("health_check", { siteName });
            setModoCliente(resultado.modo);
            setEstados((actual) => ({ ...actual, [siteName]: estadoDesdeSalud(resultado.datos) }));
        } catch (err) {
            const mensaje = err instanceof Error ? err.message : String(err);
            setEstados((actual) => ({
                ...actual,
                [siteName]: {
                    estado: "offline",
                    statusCode: null,
                    actualizado: new Date().toISOString(),
                    detalle: mensaje,
                },
            }));
        }
    }, []);

    const refrescarEstados = useCallback(async (lista: SitioResumen[] = sitios) => {
        for (const sitio of lista) {
            await refrescarEstadoSitio(sitio.name);
        }
    }, [refrescarEstadoSitio, sitios]);

    const cargarSitios = useCallback(async () => {
        setCargandoSitios(true);
        setError(null);
        try {
            const resultado = await ejecutarComandoGui<RespuestaSitios>("list_sites");
            setModoCliente(resultado.modo);
            setSitios(resultado.datos.sites);
            for (const sitio of resultado.datos.sites) {
                await refrescarEstadoSitio(sitio.name);
            }
        } catch (err) {
            setError(err instanceof Error ? err.message : String(err));
        } finally {
            setCargandoSitios(false);
        }
    }, [refrescarEstadoSitio]);

    const abrirBackups = useCallback(async (siteName: string) => {
        setSitioBackupsActivo(siteName);
        setCargandoBackups(true);
        try {
            const resultado = await ejecutarComandoGui<RespuestaBackups>("list_backups", { siteName });
            setModoCliente(resultado.modo);
            setBackups(resultado.datos);
        } catch (err) {
            setBackups({ site_name: siteName, backups: [] });
            setError(err instanceof Error ? err.message : String(err));
        } finally {
            setCargandoBackups(false);
        }
    }, []);

    useEffect(() => {
        void cargarSitios();
    }, [cargarSitios]);

    useEffect(() => {
        const intervalo = window.setInterval(() => {
            void refrescarEstados();
        }, 60_000);

        return () => window.clearInterval(intervalo);
    }, [refrescarEstados]);

    const sitiosFiltrados = useMemo(() => {
        const query = busqueda.trim().toLowerCase();
        if (!query) {
            return sitios;
        }

        return sitios.filter((sitio) => [sitio.name, sitio.domain, sitio.target, sitio.template]
            .some((value) => value.toLowerCase().includes(query)));
    }, [busqueda, sitios]);

    return {
        backups,
        busqueda,
        cargandoBackups,
        cargandoSitios,
        error,
        estados,
        modoCliente,
        sitioBackupsActivo,
        sitios,
        sitiosFiltrados,
        abrirBackups,
        cargarSitios,
        refrescarEstadoSitio,
        refrescarEstados,
        setBusqueda,
    };
}