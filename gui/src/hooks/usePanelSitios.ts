import { useCallback, useEffect, useMemo, useState } from "react";
import { ejecutarComandoGui, type ModoCliente } from "../servicios/clienteCoolify";
import type {
    MetricaDespliegue,
    RespuestaBackups,
    RespuestaLogs,
    RespuestaMetricasDespliegue,
    RespuestaSalud,
    RespuestaSitios,
    ResultadoOperacion,
    SitioResumen,
} from "../tipos";

export type EstadoOperativo = "online" | "offline" | "checking" | "unknown";

export interface EstadoSitio {
    estado: EstadoOperativo;
    statusCode: number | null;
    actualizado: string | null;
    detalle: string;
}

export interface MensajeOperacion {
    tipo: "info" | "ok" | "error";
    mensaje: string;
    detalle?: string | null;
}

const estadoInicial: EstadoSitio = {
    estado: "unknown",
    statusCode: null,
    actualizado: null,
    detalle: "Sin verificar",
};

async function ejecutarEnLotes<T>(items: T[], tamanoLote: number, tarea: (item: T) => Promise<void>) {
    for (let indice = 0; indice < items.length; indice += tamanoLote) {
        await Promise.allSettled(items.slice(indice, indice + tamanoLote).map(tarea));
    }
}

/* [105A-28] Sitios renderiza tras list_sites y deja health-check en lotes en segundo plano.
 * Gotcha: esperar cada SSH health secuencial hacia que "listar sitios" pareciera lento. */

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
    const [modoCliente, setModoCliente] = useState<ModoCliente>("local");
    const [cargandoSitios, setCargandoSitios] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [busqueda, setBusqueda] = useState("");
    const [estados, setEstados] = useState<Record<string, EstadoSitio>>({});
    const [sitioBackupsActivo, setSitioBackupsActivo] = useState<string | null>(null);
    const [backups, setBackups] = useState<RespuestaBackups | null>(null);
    const [cargandoBackups, setCargandoBackups] = useState(false);
    const [metricas, setMetricas] = useState<Record<string, MetricaDespliegue>>({});
    const [cargandoMetricas, setCargandoMetricas] = useState(false);
    const [logs, setLogs] = useState<RespuestaLogs | null>(null);
    const [cargandoLogs, setCargandoLogs] = useState(false);
    const [operacion, setOperacion] = useState<MensajeOperacion | null>(null);

    const refrescarEstadoSitio = useCallback(async (siteName: string, force = false) => {
        setEstados((actual) => ({
            ...actual,
            [siteName]: { ...(actual[siteName] ?? estadoInicial), estado: "checking" },
        }));

        try {
            const resultado = await ejecutarComandoGui<RespuestaSalud>("health_check", { siteName, force });
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

    const refrescarEstados = useCallback(async (lista: SitioResumen[] = sitios, force = false) => {
        await ejecutarEnLotes(lista, 3, (sitio) => refrescarEstadoSitio(sitio.name, force));
    }, [refrescarEstadoSitio, sitios]);

    const refrescarMetricas = useCallback(async (force = false) => {
        setCargandoMetricas(true);
        try {
            const resultado = await ejecutarComandoGui<RespuestaMetricasDespliegue>("deployment_metrics", { force });
            setModoCliente(resultado.modo);
            setMetricas(Object.fromEntries(resultado.datos.metrics.map((metrica) => [metrica.site_name, metrica])));
        } catch (err) {
            setOperacion({
                tipo: "error",
                mensaje: "No se pudieron actualizar CPU/RAM de los despliegues",
                detalle: err instanceof Error ? err.message : String(err),
            });
        } finally {
            setCargandoMetricas(false);
        }
    }, []);

    const cargarSitios = useCallback(async () => {
        setCargandoSitios(true);
        setError(null);
        try {
            const resultado = await ejecutarComandoGui<RespuestaSitios>("list_sites");
            setModoCliente(resultado.modo);
            setSitios(resultado.datos.sites);
            void ejecutarEnLotes(resultado.datos.sites, 3, (sitio) => refrescarEstadoSitio(sitio.name));
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

    const verLogs = useCallback(async (siteName: string, containerTarget: string) => {
        setCargandoLogs(true);
        setLogs(null);
        try {
            const resultado = await ejecutarComandoGui<RespuestaLogs>("view_logs", {
                siteName,
                lines: 120,
                containerTarget,
            });
            setModoCliente(resultado.modo);
            setLogs(resultado.datos);
        } catch (err) {
            setOperacion({
                tipo: "error",
                mensaje: `No se pudieron cargar registros de ${siteName}`,
                detalle: err instanceof Error ? err.message : String(err),
            });
        } finally {
            setCargandoLogs(false);
        }
    }, []);

    const ejecutarOperacionSitio = useCallback(async (
        siteName: string,
        comando: "manual_backup" | "restart_site" | "redeploy_site",
        mensajeInicio: string,
    ) => {
        setOperacion({ tipo: "info", mensaje: mensajeInicio });
        try {
            const resultado = await ejecutarComandoGui<ResultadoOperacion>(comando, { siteName });
            setModoCliente(resultado.modo);
            setOperacion({
                tipo: resultado.datos.success ? "ok" : "error",
                mensaje: resultado.datos.message,
                detalle: resultado.datos.details,
            });
            await refrescarEstadoSitio(siteName, true);
            if (comando === "manual_backup") {
                await abrirBackups(siteName);
            }
        } catch (err) {
            setOperacion({
                tipo: "error",
                mensaje: `Falló la operación sobre ${siteName}`,
                detalle: err instanceof Error ? err.message : String(err),
            });
        }
    }, [abrirBackups, refrescarEstadoSitio]);

    const crearBackupManual = useCallback((siteName: string) => {
        if (!window.confirm(`Crear una copia manual de ${siteName}?`)) {
            return;
        }
        void ejecutarOperacionSitio(siteName, "manual_backup", `Creando copia manual de ${siteName}...`);
    }, [ejecutarOperacionSitio]);

    const reiniciarSitio = useCallback((siteName: string) => {
        if (!window.confirm(`Reiniciar ${siteName} desde Coolify?`)) {
            return;
        }
        void ejecutarOperacionSitio(siteName, "restart_site", `Solicitando reinicio de ${siteName}...`);
    }, [ejecutarOperacionSitio]);

    const redeploySitio = useCallback((siteName: string) => {
        if (!window.confirm(`Ejecutar redespliegue protegido de ${siteName}?`)) {
            return;
        }
        void ejecutarOperacionSitio(siteName, "redeploy_site", `Ejecutando redespliegue protegido de ${siteName}...`);
    }, [ejecutarOperacionSitio]);

    useEffect(() => {
        void cargarSitios();
    }, [cargarSitios]);

    useEffect(() => {
        const intervalo = window.setInterval(() => {
            void refrescarEstados();
        }, 60_000);

        return () => window.clearInterval(intervalo);
    }, [refrescarEstados]);

    useEffect(() => {
        void refrescarMetricas();
        const intervalo = window.setInterval(() => {
            void refrescarMetricas();
        }, 15_000);

        return () => window.clearInterval(intervalo);
    }, [refrescarMetricas]);

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
        cargandoLogs,
        cargandoMetricas,
        error,
        estados,
        logs,
        metricas,
        modoCliente,
        operacion,
        sitioBackupsActivo,
        sitios,
        sitiosFiltrados,
        abrirBackups,
        cargarSitios,
        crearBackupManual,
        reiniciarSitio,
        redeploySitio,
        refrescarEstadoSitio,
        refrescarEstados,
        refrescarMetricas,
        setBusqueda,
        setLogs,
        verLogs,
    };
}