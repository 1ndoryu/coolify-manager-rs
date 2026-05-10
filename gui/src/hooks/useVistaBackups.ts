import { useEffect, useMemo, useState } from "react";
import { ejecutarComandoGui, type ModoCliente } from "../servicios/clienteCoolify";
import type { ErrorBackupsGlobal, RespuestaBackupsGlobal, ResumenBackupGlobal } from "../tipos";

interface UseVistaBackupsProps {
    filtroInicial: string;
    onCambiarFiltro?: (valor: string) => void;
}

export function useVistaBackups({ filtroInicial, onCambiarFiltro }: UseVistaBackupsProps) {
    const [backups, setBackups] = useState<ResumenBackupGlobal[]>([]);
    const [errores, setErrores] = useState<ErrorBackupsGlobal[]>([]);
    const [busqueda, setBusqueda] = useState(filtroInicial);
    const [modoCliente, setModoCliente] = useState<ModoCliente>("local");
    const [cargando, setCargando] = useState(true);
    const [error, setError] = useState<string | null>(null);

    const backupsFiltrados = useMemo(() => {
        const filtro = busqueda.trim().toLowerCase();
        if (!filtro) return backups;

        return backups.filter((backup) => [
            backup.site_name,
            backup.domain,
            backup.target,
            backup.template,
            backup.backup_id,
            backup.tier,
            backup.status,
            backup.label ?? "",
        ].some((valor) => valor.toLowerCase().includes(filtro)));
    }, [backups, busqueda]);

    function actualizarBusqueda(valor: string) {
        setBusqueda(valor);
        onCambiarFiltro?.(valor);
    }

    async function cargarBackups(force = false) {
        setCargando(true);
        setError(null);
        try {
            const resultado = await ejecutarComandoGui<RespuestaBackupsGlobal>("list_all_backups", { force });
            setModoCliente(resultado.modo);
            setBackups(resultado.datos.backups);
            setErrores(resultado.datos.errors);
        } catch (err) {
            setError(err instanceof Error ? err.message : String(err));
            setBackups([]);
            setErrores([]);
        } finally {
            setCargando(false);
        }
    }

    useEffect(() => {
        void cargarBackups();
    }, []);

    useEffect(() => {
        setBusqueda(filtroInicial);
    }, [filtroInicial]);

    return { backupsFiltrados, busqueda, modoCliente, cargando, error, errores, actualizarBusqueda, cargarBackups };
}