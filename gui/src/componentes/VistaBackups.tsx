import { Archive, RefreshCw, Search, X } from "lucide-react";
import { useVistaBackups } from "../hooks/useVistaBackups";
import { claseModoCliente, etiquetaModoCliente } from "../servicios/clienteCoolify";
import { Button } from "./ui/Button";

interface VistaBackupsProps {
    filtroInicial?: string;
    onCambiarFiltro?: (valor: string) => void;
}

export function VistaBackups({ filtroInicial = "", onCambiarFiltro }: VistaBackupsProps) {
    const vista = useVistaBackups({ filtroInicial, onCambiarFiltro });

    return (
        <div className="vistaConsola">
            <header className="barraSuperior">
                <div>
                    <h1 className="tituloPagina">Copias</h1>
                </div>
                <div className="accionesSuperiores">
                    <span className={`badge ${claseModoCliente(vista.modoCliente)}`}>{etiquetaModoCliente(vista.modoCliente)}</span>
                    <Button onClick={() => void vista.cargarBackups(true)}><RefreshCw size={14} /> Actualizar</Button>
                </div>
            </header>

            {vista.error && <div className="mensajeError">{vista.error}</div>}
            {vista.errores.length > 0 && (
                <div className="mensajeError">
                    {vista.errores.map((item) => <span key={item.site_name}>{item.site_name}: {item.message}</span>)}
                </div>
            )}

            <section className="panelTabla">
                <div className="toolbarTabla">
                    <div className="grupoToolbar"><Archive size={14} /> {vista.backupsFiltrados.length} copias visibles</div>
                    <div className="grupoToolbar grupoToolbarDerecha">
                        <label className="busquedaConIcono">
                            <Search size={14} />
                            <input
                                className="campoBusqueda"
                                value={vista.busqueda}
                                onChange={(event) => vista.actualizarBusqueda(event.target.value)}
                                placeholder="Buscar copias..."
                            />
                        </label>
                        {vista.busqueda && (
                            <Button onClick={() => vista.actualizarBusqueda("")}><X size={14} /> Limpiar</Button>
                        )}
                    </div>
                </div>
                {vista.cargando ? (
                    <div className="cargando bloquePanel"><div className="spinner" /> Cargando copias...</div>
                ) : vista.backupsFiltrados.length === 0 ? (
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
                                {vista.backupsFiltrados.map((backup) => (
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