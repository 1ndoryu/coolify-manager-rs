/*
 * VistaSitios — consola principal de servicios.
 */

import { Archive, ExternalLink, RefreshCw, RotateCcw, Server, ShieldCheck, Terminal, UploadCloud } from "lucide-react";
import type { EstadoSitio } from "../hooks/usePanelSitios";
import { useVistaSitios } from "../hooks/useVistaSitios";
import { claseModoCliente, etiquetaModoCliente } from "../servicios/clienteCoolify";
import type { MetricaDespliegue, SitioResumen, TargetResumen } from "../tipos";
import { MetricaCpu, MetricaRam } from "./MetricasSitio";
import { ModalAgregarSitio } from "./ModalAgregarSitio";
import { Button } from "./ui/Button";
import { MenuContextual, type AccionMenu } from "./ui/MenuContextual";
import "./VistaSitios.css";

interface VistaSitiosProps {
    targets: TargetResumen[];
    targetActivo: string;
    onVerCopiasSitio: (siteName: string) => void;
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

function faviconSitio(sitio: SitioResumen): string | null {
    try {
        return new URL("/favicon.ico", sitio.domain).toString();
    } catch {
        return null;
    }
}

export function VistaSitios({ targets, targetActivo, onVerCopiasSitio }: VistaSitiosProps) {
    /* [105A-17..24] Tabla operativa en español: acciones en menu, CPU/RAM real y verificacion relativa.
     * Gotcha: navegador y Tauri comparten API real; demo solo existe si se fuerza por variable de entorno. */
    const vista = useVistaSitios();
    const { panel } = vista;

    return (
        <div className="vistaConsola">
            <header className="barraSuperior">
                <div>
                    <h1 className="tituloPagina">Lista de sitios</h1>
                </div>
                <div className="accionesSuperiores">
                    <span className={`badge ${claseModoCliente(panel.modoCliente)}`}>
                        {etiquetaModoCliente(panel.modoCliente)}
                    </span>
                    <Button onClick={() => void panel.refrescarEstados(undefined, true)}>
                        <RefreshCw size={14} /> Verificar estado
                    </Button>
                    <Button variant="primario" onClick={vista.abrirModalAgregar}>
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
                        <span>En línea {vista.conteos.online}</span>
                        <span>Incidencias {vista.conteos.issues}</span>
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
                                    onAbrirBackups={() => onVerCopiasSitio(sitio.name)}
                                    onRefresh={() => void panel.refrescarEstadoSitio(sitio.name, true)}
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

            {(panel.logs || panel.cargandoLogs) && (
                <section className="panelBackups">
                    <div className="cabeceraPanelSecundario">
                        <div>
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
            <ModalAgregarSitio
                abierto={vista.modalAgregarAbierto}
                targets={targets}
                targetActivo={targetActivo}
                onCerrar={vista.cerrarModalAgregar}
                onCreado={vista.confirmarSitioCreado}
            />
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
    const favicon = faviconSitio(sitio);
    const inicial = sitio.name.slice(0, 1).toUpperCase();
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
                    <span className="avatarServicio" aria-hidden="true">
                        <span className="avatarServicioFallback">{inicial}</span>
                        {favicon && <img src={favicon} alt="" loading="lazy" onError={(event) => { event.currentTarget.hidden = true; }} />}
                    </span>
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

/* [105A-27] La tabla consume metricas reales via Tauri o gui-api; si no hay contenedor, no se inventan valores. */
