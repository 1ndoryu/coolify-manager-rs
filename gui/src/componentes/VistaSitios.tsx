/*
 * VistaSitios — consola principal de servicios.
 */

import { Activity, Archive, ExternalLink, RefreshCw, RotateCcw, Server, ShieldCheck, Terminal, UploadCloud } from "lucide-react";
import { usePanelSitios, type EstadoSitio } from "../hooks/usePanelSitios";
import type { MetricaDespliegue, ResumenBackup, SitioResumen } from "../tipos";
import { Button } from "./ui/Button";
import { MenuContextual, type AccionMenu } from "./ui/MenuContextual";

interface VistaSitiosProps {
    onAgregarSitio: () => void;
}

function formatearFechaRelativa(valor: string | null): string {
    if (!valor) {
        return "Sin verificar";
    }

    const diferencia = Date.now() - new Date(valor).getTime();
    if (diferencia < 45_000) return "Ahora";
    const minutos = Math.floor(diferencia / 60_000);
    if (minutos < 60) return `Hace ${minutos} min`;
    const horas = Math.floor(minutos / 60);
    if (horas < 24) return `Hace ${horas} h`;
    return new Intl.DateTimeFormat("es-ES", { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" }).format(new Date(valor));
}

function formatearBytes(bytes: number): string {
    if (!Number.isFinite(bytes) || bytes <= 0) {
        return "--";
    }

    const unidades = ["B", "KB", "MB", "GB", "TB"];
    let valor = bytes;
    let indice = 0;
    while (valor >= 1024 && indice < unidades.length - 1) {
        valor /= 1024;
        indice += 1;
    }

    return `${valor >= 10 ? valor.toFixed(0) : valor.toFixed(1)} ${unidades[indice]}`;
}

function claseEstado(estado: EstadoSitio): string {
    if (estado.estado === "online") return "badgeExito";
    if (estado.estado === "offline") return "badgeError";
    if (estado.estado === "checking") return "badgeAdvertencia";
    return "badgeNeutro";
}

function etiquetaEstado(estado: EstadoSitio): string {
    if (estado.estado === "online") return "En línea";
    if (estado.estado === "offline") return "Incidencia";
    if (estado.estado === "checking") return "Verificando";
    return "Sin datos";
}

function claseOperacion(tipo: "info" | "ok" | "error"): string {
    if (tipo === "ok") return "mensajeOperacionOk";
    if (tipo === "error") return "mensajeOperacionError";
    return "mensajeOperacionInfo";
}

function targetLogsParaSitio(sitio: SitioResumen): string {
    return sitio.template.toLowerCase().includes("rust") ? "app" : "wordpress";
}

export function VistaSitios({ onAgregarSitio }: VistaSitiosProps) {
    /* [105A-17..22] Tabla operativa en español: acciones en menu, CPU/RAM real y verificacion relativa.
     * Gotcha: el modo navegador es solo preview; `npm run dev` abre Tauri y ejecuta operaciones reales. */
    const panel = usePanelSitios();
    const conteoOnline = panel.sitios.filter((sitio) => panel.estados[sitio.name]?.estado === "online").length;
    const conteoIssues = panel.sitios.filter((sitio) => panel.estados[sitio.name]?.estado === "offline").length;

    return (
        <div className="vistaConsola">
            <header className="barraSuperior">
                <div>
                    <div className="rutaPagina">Coolify / Sitios</div>
                    <h1 className="tituloPagina">Lista de sitios</h1>
                </div>
                <div className="accionesSuperiores">
                    <span className={`badge ${panel.modoCliente === "tauri" ? "badgeExito" : "badgeNeutro"}`}>
                        {panel.modoCliente === "tauri" ? "Modo real" : "Modo navegador"}
                    </span>
                    <Button onClick={() => void panel.refrescarEstados()}>
                        <RefreshCw size={14} /> Verificar estado
                    </Button>
                    <Button variant="primario" onClick={onAgregarSitio}>
                        + Agregar sitio
                    </Button>
                </div>
            </header>

            <section className="panelTabla">
                <div className="toolbarTabla">
                    <div className="grupoToolbar">
                        <Server size={14} />
                        <span>Servicios · {panel.sitios.length}</span>
                        {panel.cargandoMetricas && <span className="textoSuave">Actualizando CPU/RAM...</span>}
                    </div>
                    <div className="grupoToolbar grupoToolbarDerecha">
                        <span>En línea {conteoOnline}</span>
                        <span>Incidencias {conteoIssues}</span>
                        <input
                            className="campoBusqueda"
                            value={panel.busqueda}
                            onChange={(event) => panel.setBusqueda(event.target.value)}
                            placeholder="Buscar sitios..."
                        />
                    </div>
                </div>

                {panel.operacion && (
                    <div className={`mensajeOperacion ${claseOperacion(panel.operacion.tipo)}`}>
                        <strong>{panel.operacion.mensaje}</strong>
                        {panel.operacion.detalle && <span>{panel.operacion.detalle}</span>}
                    </div>
                )}
                {panel.error && <div className="mensajeError">{panel.error}</div>}

                <div className="contenedorTabla">
                    <table className="tabla tablaServicios">
                        <thead>
                            <tr>
                                <th><span className="checkFantasma" /></th>
                                <th>Nombre</th>
                                <th>Estado</th>
                                <th>CPU</th>
                                <th>RAM</th>
                                <th>Dominio</th>
                                <th>VPS</th>
                                <th>Stack UUID</th>
                                <th>Plantilla</th>
                                <th>Última verificación</th>
                                <th>Acciones</th>
                            </tr>
                        </thead>
                        <tbody>
                            {panel.cargandoSitios ? (
                                <tr>
                                    <td colSpan={11}>
                                        <div className="cargando"><div className="spinner" /> Cargando sitios...</div>
                                    </td>
                                </tr>
                            ) : panel.sitiosFiltrados.map((sitio) => (
                                <FilaSitio
                                    key={sitio.name}
                                    sitio={sitio}
                                    estado={panel.estados[sitio.name]}
                                    metrica={panel.metricas[sitio.name]}
                                    onAbrirBackups={() => void panel.abrirBackups(sitio.name)}
                                    onRefresh={() => void panel.refrescarEstadoSitio(sitio.name)}
                                    onVerLogs={() => void panel.verLogs(sitio.name, targetLogsParaSitio(sitio))}
                                    onBackupManual={() => panel.crearBackupManual(sitio.name)}
                                    onReiniciar={() => panel.reiniciarSitio(sitio.name)}
                                    onRedeploy={() => panel.redeploySitio(sitio.name)}
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
                            <div className="rutaPagina">Copias</div>
                            <h2 className="tituloPanelSecundario">{panel.sitioBackupsActivo}</h2>
                        </div>
                        <Button onClick={() => panel.crearBackupManual(panel.sitioBackupsActivo ?? "")}>
                            <UploadCloud size={14} /> Copia manual
                        </Button>
                    </div>
                    {panel.cargandoBackups ? (
                        <div className="cargando bloquePanel"><div className="spinner" /> Cargando copias...</div>
                    ) : (
                        <TablaBackups backups={panel.backups?.backups ?? []} />
                    )}
                </section>
            )}

            {(panel.logs || panel.cargandoLogs) && (
                <section className="panelBackups">
                    <div className="cabeceraPanelSecundario">
                        <div>
                            <div className="rutaPagina">Registros</div>
                            <h2 className="tituloPanelSecundario">{panel.logs?.site_name ?? "Cargando"}</h2>
                        </div>
                        <Button onClick={() => panel.setLogs(null)}>Cerrar</Button>
                    </div>
                    {panel.cargandoLogs ? (
                        <div className="cargando bloquePanel"><div className="spinner" /> Cargando registros...</div>
                    ) : (
                        <pre className="panelLogs">{panel.logs?.content || panel.logs?.stderr || "Sin registros disponibles."}</pre>
                    )}
                </section>
            )}
        </div>
    );
}

function FilaSitio({
    sitio,
    estado,
    metrica,
    onAbrirBackups,
    onRefresh,
    onVerLogs,
    onBackupManual,
    onReiniciar,
    onRedeploy,
}: {
    sitio: SitioResumen;
    estado?: EstadoSitio;
    metrica?: MetricaDespliegue;
    onAbrirBackups: () => void;
    onRefresh: () => void;
    onVerLogs: () => void;
    onBackupManual: () => void;
    onReiniciar: () => void;
    onRedeploy: () => void;
}) {
    const estadoFila = estado ?? { estado: "unknown", statusCode: null, actualizado: null, detalle: "Sin verificar" };
    const acciones: AccionMenu[] = [
        { etiqueta: "Abrir sitio", icono: <ExternalLink size={14} />, onClick: () => window.open(sitio.domain, "_blank", "noopener,noreferrer") },
        { etiqueta: "Verificar estado", icono: <RefreshCw size={14} />, onClick: onRefresh },
        { etiqueta: "Ver copias", icono: <Archive size={14} />, onClick: onAbrirBackups },
        { etiqueta: "Ver registros", icono: <Terminal size={14} />, onClick: onVerLogs },
        { etiqueta: "Copia manual", icono: <UploadCloud size={14} />, onClick: onBackupManual },
        { etiqueta: "Reiniciar", icono: <RotateCcw size={14} />, onClick: onReiniciar },
        { etiqueta: "Redespliegue protegido", icono: <ShieldCheck size={14} />, tono: "peligro", onClick: onRedeploy },
    ];

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
            <td><MetricaCpu metrica={metrica} /></td>
            <td><MetricaRam metrica={metrica} /></td>
            <td><span className="pildoraDominio">{sitio.domain.replace(/^https?:\/\//, "")}</span></td>
            <td>{sitio.target}</td>
            <td><span className="textoMono textoCorto">{sitio.stack_uuid}</span></td>
            <td>{sitio.template}</td>
            <td>{formatearFechaRelativa(estadoFila.actualizado)}</td>
            <td><MenuContextual etiqueta={`Acciones de ${sitio.name}`} acciones={acciones} /></td>
        </tr>
    );
}

function MetricaCpu({ metrica }: { metrica?: MetricaDespliegue }) {
    if (!metrica || metrica.status !== "running") {
        return <span className="textoSuave">--</span>;
    }

    return (
        <span className="metricaCompacta" title={`${metrica.containers.length} contenedor(es)`}>
            <Activity size={13} /> {metrica.total_cpu_percent.toFixed(1)}%
        </span>
    );
}

function MetricaRam({ metrica }: { metrica?: MetricaDespliegue }) {
    if (!metrica || metrica.status !== "running") {
        return <span className="textoSuave">--</span>;
    }

    return (
        <div className="metricaRam" title={`${formatearBytes(metrica.memory_used_bytes)} / ${formatearBytes(metrica.memory_limit_bytes)}`}>
            <meter className="barraMetrica" value={Math.min(metrica.memory_percent, 100)} max={100} />
            <span>{formatearBytes(metrica.memory_used_bytes)}</span>
        </div>
    );
}

function TablaBackups({ backups }: { backups: ResumenBackup[] }) {
    if (backups.length === 0) {
        return <div className="estadoVacio">No hay copias para este sitio.</div>;
    }

    return (
        <div className="contenedorTabla">
            <table className="tabla">
                <thead>
                    <tr>
                        <th>ID</th>
                        <th>Tipo</th>
                        <th>Estado</th>
                        <th>Fecha</th>
                        <th>Etiqueta</th>
                        <th>Artefactos</th>
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