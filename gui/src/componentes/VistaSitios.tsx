/*
 * VistaSitios — consola principal de servicios.
 */

import { Archive, ExternalLink, MoreHorizontal, RefreshCw, RotateCcw, Server, ShieldCheck, Terminal, UploadCloud } from "lucide-react";
import { usePanelSitios, type EstadoSitio } from "../hooks/usePanelSitios";
import type { ResumenBackup, SitioResumen } from "../tipos";
import { Button, IconButton } from "./ui/Button";

function formatearFecha(valor: string | null): string {
    if (!valor) {
        return "--";
    }

    return new Intl.DateTimeFormat("es-ES", {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
    }).format(new Date(valor));
}

function claseEstado(estado: EstadoSitio): string {
    if (estado.estado === "online") return "badgeExito";
    if (estado.estado === "offline") return "badgeError";
    if (estado.estado === "checking") return "badgeAdvertencia";
    return "badgeNeutro";
}

function etiquetaEstado(estado: EstadoSitio): string {
    if (estado.estado === "online") return "Online";
    if (estado.estado === "offline") return "Issue";
    if (estado.estado === "checking") return "Checking";
    return "Unknown";
}

export function VistaSitios() {
    const panel = usePanelSitios();
    const conteoOnline = panel.sitios.filter((sitio) => panel.estados[sitio.name]?.estado === "online").length;
    const conteoIssues = panel.sitios.filter((sitio) => panel.estados[sitio.name]?.estado === "offline").length;

    return (
        <div className="vistaConsola">
            <header className="barraSuperior">
                <div>
                    <div className="rutaPagina">Coolify / Services</div>
                    <h1 className="tituloPagina">Sites</h1>
                </div>
                <div className="accionesSuperiores">
                    <span className={`badge ${panel.modoCliente === "tauri" ? "badgeExito" : "badgeNeutro"}`}>
                        {panel.modoCliente === "tauri" ? "Native runtime" : "Browser mode"}
                    </span>
                    <Button onClick={() => void panel.refrescarEstados()}>
                        <RefreshCw size={14} /> Refresh status
                    </Button>
                    <Button variant="primario" title="Fase 2: crear sitio desde GUI">
                        + New record
                    </Button>
                </div>
            </header>

            <section className="panelTabla">
                <div className="toolbarTabla">
                    <div className="grupoToolbar">
                        <Server size={14} />
                        <span>All services · {panel.sitios.length}</span>
                    </div>
                    <div className="grupoToolbar grupoToolbarDerecha">
                        <span>Online {conteoOnline}</span>
                        <span>Issues {conteoIssues}</span>
                        <input
                            className="campoBusqueda"
                            value={panel.busqueda}
                            onChange={(event) => panel.setBusqueda(event.target.value)}
                            placeholder="Search services..."
                        />
                    </div>
                </div>

                {panel.error && <div className="mensajeError">{panel.error}</div>}

                <div className="contenedorTabla">
                    <table className="tabla tablaServicios">
                        <thead>
                            <tr>
                                <th><span className="checkFantasma" /></th>
                                <th>Name</th>
                                <th>Status</th>
                                <th>Domain</th>
                                <th>Target</th>
                                <th>Stack UUID</th>
                                <th>Template</th>
                                <th>Updated</th>
                                <th>Actions</th>
                            </tr>
                        </thead>
                        <tbody>
                            {panel.cargandoSitios ? (
                                <tr>
                                    <td colSpan={9}>
                                        <div className="cargando"><div className="spinner" /> Loading services...</div>
                                    </td>
                                </tr>
                            ) : panel.sitiosFiltrados.map((sitio) => (
                                <FilaSitio
                                    key={sitio.name}
                                    sitio={sitio}
                                    estado={panel.estados[sitio.name]}
                                    onBackups={() => void panel.abrirBackups(sitio.name)}
                                    onRefresh={() => void panel.refrescarEstadoSitio(sitio.name)}
                                />
                            ))}
                        </tbody>
                    </table>
                </div>
            </section>

            {panel.sitioBackupsActivo && (
                <section className="panelBackups">
                    <div className="cabeceraPanelSecundario">
                        <div>
                            <div className="rutaPagina">Backups</div>
                            <h2 className="tituloPanelSecundario">{panel.sitioBackupsActivo}</h2>
                        </div>
                        <Button title="Fase 2: backup manual nativo">
                            <UploadCloud size={14} /> Manual backup
                        </Button>
                    </div>
                    {panel.cargandoBackups ? (
                        <div className="cargando"><div className="spinner" /> Loading backups...</div>
                    ) : (
                        <TablaBackups backups={panel.backups?.backups ?? []} />
                    )}
                </section>
            )}
        </div>
    );
}

function FilaSitio({ sitio, estado, onBackups, onRefresh }: {
    sitio: SitioResumen;
    estado?: EstadoSitio;
    onBackups: () => void;
    onRefresh: () => void;
}) {
    const estadoFila = estado ?? { estado: "unknown", statusCode: null, actualizado: null, detalle: "Sin verificar" };

    return (
        <tr>
            <td><span className="checkFantasma" /></td>
            <td>
                <div className="celdaNombre">
                    <span className="avatarServicio">{sitio.name.slice(0, 1).toUpperCase()}</span>
                    <span>{sitio.name}</span>
                </div>
            </td>
            <td>
                <span className={`badge ${claseEstado(estadoFila)}`} title={estadoFila.detalle}>
                    {etiquetaEstado(estadoFila)}{estadoFila.statusCode ? ` · ${estadoFila.statusCode}` : ""}
                </span>
            </td>
            <td><span className="pildoraDominio">{sitio.domain.replace(/^https?:\/\//, "")}</span></td>
            <td>{sitio.target}</td>
            <td><span className="textoMono textoCorto">{sitio.stack_uuid}</span></td>
            <td>{sitio.template}</td>
            <td>{formatearFecha(estadoFila.actualizado)}</td>
            <td>
                <div className="accionesFila">
                    <IconButton title="Open site" onClick={() => window.open(sitio.domain, "_blank", "noopener,noreferrer")} icon={<ExternalLink size={14} />} />
                    <IconButton title="Refresh status" onClick={onRefresh} icon={<RefreshCw size={14} />} />
                    <IconButton title="List backups" onClick={onBackups} icon={<Archive size={14} />} />
                    <IconButton className="botonIconoPendiente" title="Fase 2: view logs" icon={<Terminal size={14} />} />
                    <IconButton className="botonIconoPendiente" title="Fase 2: restart" icon={<RotateCcw size={14} />} />
                    <IconButton className="botonIconoPendiente" title="Fase 2: protected redeploy" icon={<ShieldCheck size={14} />} />
                    <IconButton className="botonIconoPendiente" title="More actions" icon={<MoreHorizontal size={14} />} />
                </div>
            </td>
        </tr>
    );
}

function TablaBackups({ backups }: { backups: ResumenBackup[] }) {
    if (backups.length === 0) {
        return <div className="estadoVacio">No backups found.</div>;
    }

    return (
        <div className="contenedorTabla">
            <table className="tabla">
                <thead>
                    <tr>
                        <th>ID</th>
                        <th>Tier</th>
                        <th>Status</th>
                        <th>Date</th>
                        <th>Label</th>
                        <th>Artifacts</th>
                    </tr>
                </thead>
                <tbody>
                    {backups.map((backup) => (
                        <tr key={backup.backup_id}>
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
    );
}
