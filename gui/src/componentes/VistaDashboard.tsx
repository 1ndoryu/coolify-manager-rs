import { Activity, Database, HardDrive, RefreshCw, Server } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { claseModoCliente, ejecutarComandoGui, etiquetaModoCliente, type ModoCliente } from "../servicios/clienteCoolify";
import type { RespuestaAuditoria, RespuestaTargets, TargetResumen } from "../tipos";
import { Button } from "./ui/Button";

function porcentajeMemoria(auditoria: RespuestaAuditoria | null): number {
    if (!auditoria?.memory_total_mb || !auditoria.memory_used_mb) {
        return 0;
    }

    return (auditoria.memory_used_mb / auditoria.memory_total_mb) * 100;
}

function valorPorcentaje(valor: number | null | undefined): number {
    return Math.max(0, Math.min(valor ?? 0, 100));
}

export function VistaDashboard() {
    const [targets, setTargets] = useState<TargetResumen[]>([]);
    const [targetActivo, setTargetActivo] = useState("default");
    const [auditoria, setAuditoria] = useState<RespuestaAuditoria | null>(null);
    const [modoCliente, setModoCliente] = useState<ModoCliente>("local");
    const [cargando, setCargando] = useState(true);
    const [error, setError] = useState<string | null>(null);

    const targetSeleccionado = useMemo(
        () => targets.find((target) => target.name === targetActivo) ?? targets[0],
        [targetActivo, targets],
    );

    async function cargarTargets() {
        const resultado = await ejecutarComandoGui<RespuestaTargets>("list_targets");
        setModoCliente(resultado.modo);
        setTargets(resultado.datos.targets);
        setTargetActivo(resultado.datos.default_target || resultado.datos.targets[0]?.name || "default");
    }

    async function cargarAuditoria(target: string, force = false) {
        setCargando(true);
        setError(null);
        try {
            const resultado = await ejecutarComandoGui<RespuestaAuditoria>("audit_vps", { target, force });
            setModoCliente(resultado.modo);
            setAuditoria(resultado.datos);
        } catch (err) {
            setError(err instanceof Error ? err.message : String(err));
        } finally {
            setCargando(false);
        }
    }

    useEffect(() => {
        void cargarTargets();
    }, []);

    useEffect(() => {
        if (targetActivo) {
            void cargarAuditoria(targetActivo);
        }
    }, [targetActivo]);

    return (
        <div className="vistaConsola">
            <header className="barraSuperior">
                <div>
                    <div className="rutaPagina">Coolify / Panel</div>
                    <h1 className="tituloPagina">Estado de VPS</h1>
                </div>
                <div className="accionesSuperiores">
                    <span className={`badge ${claseModoCliente(modoCliente)}`}>
                        {etiquetaModoCliente(modoCliente)}
                    </span>
                    <select className="selectorCompacto" value={targetActivo} onChange={(event) => setTargetActivo(event.target.value)}>
                        {targets.length === 0 && <option value="default">Cargando VPS...</option>}
                        {targets.map((target) => <option key={target.name} value={target.name}>{target.name}</option>)}
                    </select>
                    <Button onClick={() => void cargarAuditoria(targetActivo, true)}><RefreshCw size={14} /> Actualizar</Button>
                </div>
            </header>

            {error && <div className="mensajeError">{error}</div>}

            <section className="gridMetricas">
                <TarjetaMetrica icono={<Server size={15} />} etiqueta="VPS activo" valor={targetSeleccionado?.host ?? "--"} detalle={targetSeleccionado ? `${targetSeleccionado.site_count} sitios · ${targetSeleccionado.user}` : "Sin destino"} />
                <TarjetaMetrica icono={<Activity size={15} />} etiqueta="Carga CPU" valor={auditoria?.load_1m?.toFixed(2) ?? "--"} detalle={auditoria?.load_average ?? "Sin lectura"} />
                <TarjetaMetrica icono={<Database size={15} />} etiqueta="RAM" valor={`${porcentajeMemoria(auditoria).toFixed(0)}%`} detalle={auditoria?.memory_summary ?? "Sin lectura"} />
                <TarjetaMetrica icono={<HardDrive size={15} />} etiqueta="Disco" valor={`${valorPorcentaje(auditoria?.disk_use_percent).toFixed(0)}%`} detalle={auditoria?.disk_summary ?? "Sin lectura"} />
            </section>

            <section className="gridDosColumnas">
                <div className="tarjeta">
                    <h2 className="tarjetaTitulo">VPS configurados</h2>
                    <div className="listaTargets">
                        {targets.map((target) => (
                            <button
                                key={target.name}
                                className={`filaTarget ${target.name === targetActivo ? "filaTargetActiva" : ""}`}
                                type="button"
                                onClick={() => setTargetActivo(target.name)}
                            >
                                <span>{target.name}</span>
                                <strong>{target.host}</strong>
                                <small>{target.coolify_url}</small>
                            </button>
                        ))}
                    </div>
                </div>
                <div className="tarjeta">
                    <h2 className="tarjetaTitulo">Lecturas del servidor</h2>
                    {cargando ? (
                        <div className="cargando"><div className="spinner" /> Leyendo VPS...</div>
                    ) : (
                        <div className="listaLecturas">
                            <Lectura etiqueta="Docker" valor={auditoria?.docker_summary || "Sin contenedores detectados"} />
                            <Lectura etiqueta="Seguridad" valor={auditoria?.security_summary || "Sin datos"} />
                            <Lectura etiqueta="Recomendaciones" valor={auditoria?.recommendations.length ? auditoria.recommendations.join("\n") : "Sin recomendaciones críticas"} />
                        </div>
                    )}
                </div>
            </section>
        </div>
    );
}

function TarjetaMetrica({ icono, etiqueta, valor, detalle }: { icono: ReactNode; etiqueta: string; valor: string; detalle: string }) {
    return (
        <article className="tarjetaMetrica">
            <div className="metricaIcono">{icono}</div>
            <span>{etiqueta}</span>
            <strong>{valor}</strong>
            <small>{detalle}</small>
        </article>
    );
}

function Lectura({ etiqueta, valor }: { etiqueta: string; valor: string }) {
    return (
        <div className="filaLectura">
            <span>{etiqueta}</span>
            <pre>{valor}</pre>
        </div>
    );
}