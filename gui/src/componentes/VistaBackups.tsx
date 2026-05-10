import { Archive, RefreshCw, Search, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { claseModoCliente, ejecutarComandoGui, etiquetaModoCliente, type ModoCliente } from "../servicios/clienteCoolify";
import type { ErrorBackupsGlobal, RespuestaBackupsGlobal, ResumenBackupGlobal } from "../tipos";
import { Button } from "./ui/Button";

interface VistaBackupsProps {
    filtroInicial?: string;
    onCambiarFiltro?: (valor: string) => void;
}

export function VistaBackups({ filtroInicial = "", onCambiarFiltro }: VistaBackupsProps) {
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

    return (
        <div className="vistaConsola">
            <header className="barraSuperior">
                <div>
                    <div className="rutaPagina">Coolify / Copias</div>
                    <h1 className="tituloPagina">Copias</h1>
                </div>
                <div className="accionesSuperiores">
                    <span className={`badge ${claseModoCliente(modoCliente)}`}>{etiquetaModoCliente(modoCliente)}</span>
                    <Button onClick={() => void cargarBackups(true)}><RefreshCw size={14} /> Actualizar</Button>
                </div>
            </header>

            {error && <div className="mensajeError">{error}</div>}
            {errores.length > 0 && (
                <div className="mensajeError">
                    {errores.map((item) => <span key={item.site_name}>{item.site_name}: {item.message}</span>)}
                </div>
            )}

            <section className="panelTabla">
                <div className="toolbarTabla">
                    <div className="grupoToolbar"><Archive size={14} /> {backupsFiltrados.length} copias visibles</div>
                    <div className="grupoToolbar grupoToolbarDerecha">
                        <label className="busquedaConIcono">
                            <Search size={14} />
                            <input
                                className="campoBusqueda"
                                value={busqueda}
                                onChange={(event) => actualizarBusqueda(event.target.value)}
                                placeholder="Buscar copias..."
                            />
                        </label>
                        {busqueda && (
                            <Button onClick={() => actualizarBusqueda("")}><X size={14} /> Limpiar</Button>
                        )}
                    </div>
                </div>
                {cargando ? (
                    <div className="cargando bloquePanel"><div className="spinner" /> Cargando copias...</div>
                ) : backupsFiltrados.length === 0 ? (
                    <div className="estadoVacio">No hay copias que coincidan con la búsqueda.</div>
                ) : (
                    <div className="contenedorTabla">
                        <table className="tabla">
                            <thead>
                                <tr>
                                    <th>Sitio</th>
                                    <th>Dominio</th>
                                    <th>VPS</th>
                                    <th>ID</th>
                                    <th>Tipo</th>
                                    <th>Estado</th>
                                    <th>Fecha</th>
                                    <th>Etiqueta</th>
                                    <th>Artefactos</th>
                                </tr>
                            </thead>
                            <tbody>
                                {backupsFiltrados.map((backup) => (
                                    <tr key={`${backup.site_name}-${backup.backup_id}`}>
                                        <td><strong>{backup.site_name}</strong></td>
                                        <td><span className="pildoraDominio">{backup.domain.replace(/^https?:\/\//, "")}</span></td>
                                        <td>{backup.target}</td>
                                        <td><span className="textoMono textoCorto">{backup.backup_id}</span></td>
                                        <td><span className="badge badgeNeutro">{backup.tier}</span></td>
                                        <td><span className="badge badgeExito">{backup.status}</span></td>
                                        <td>{backup.created_at}</td>
                                        <td>{backup.label ?? "--"}</td>
                                        <td>{backup.artifact_count}</td>
                                    </tr>
                                ))}
                            </tbody>
                        </table>
                    </div>
                )}
            </section>
        </div>
    );
}