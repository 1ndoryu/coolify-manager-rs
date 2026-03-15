/*
 * VistaSalud — health check de un sitio.
 */

import { useState } from "react";
import { useComandoTauri } from "../hooks/useComandoTauri";
import type { RespuestaSalud } from "../tipos";

interface Props {
    sitioInicial: string | null;
}

export function VistaSalud({ sitioInicial }: Props) {
    const [sitio, setSitio] = useState(sitioInicial ?? "");
    const { datos, cargando, error, ejecutar } = useComandoTauri<RespuestaSalud>("health_check");

    const verificar = () => {
        if (!sitio.trim()) return;
        ejecutar({ siteName: sitio.trim() });
    };

    return (
        <div>
            <div className="cabeceraPagina">
                <h1 className="tituloPagina">Health Check</h1>
                <p className="subtituloPagina">Verifica el estado de un sitio</p>
            </div>

            <div className="tarjeta" style={{ marginBottom: "var(--espacioLg)" }}>
                <div style={{ display: "flex", gap: "var(--espacioSm)", alignItems: "center" }}>
                    <input
                        type="text"
                        value={sitio}
                        onChange={(e) => setSitio(e.target.value)}
                        onKeyDown={(e) => e.key === "Enter" && verificar()}
                        placeholder="Nombre del sitio (ej: kamples)"
                        style={{
                            flex: 1,
                            background: "var(--fondoElevado2)",
                            border: "1px solid var(--bordeSutil)",
                            borderRadius: "var(--radioSm)",
                            padding: "var(--espacioSm) var(--espacioMd)",
                            color: "var(--textoPrimario)",
                            fontSize: "var(--fuenteSm)",
                            fontFamily: "var(--fuenteMono)",
                            outline: "none",
                        }}
                    />
                    <button
                        className={`boton botonPrimario ${cargando ? "botonDeshabilitado" : ""}`}
                        onClick={verificar}
                    >
                        {cargando ? "Verificando..." : "Verificar"}
                    </button>
                </div>
            </div>

            {error && <div className="mensajeError">{error}</div>}

            {datos && (
                <div className="tarjeta">
                    <div style={{ display: "flex", alignItems: "center", gap: "var(--espacioMd)", marginBottom: "var(--espacioLg)" }}>
                        <span className={`badge ${datos.healthy ? "badgeExito" : "badgeError"}`}>
                            {datos.healthy ? "Saludable" : "Problemas detectados"}
                        </span>
                        <span className="textoMono">{datos.url}</span>
                        {datos.status_code && (
                            <span className="badge badgeInfo">HTTP {datos.status_code}</span>
                        )}
                    </div>

                    <table className="tabla">
                        <tbody>
                            <tr>
                                <td>HTTP OK</td>
                                <td><IndicadorEstado ok={datos.http_ok} /></td>
                            </tr>
                            <tr>
                                <td>App OK</td>
                                <td><IndicadorEstado ok={datos.app_ok} /></td>
                            </tr>
                            <tr>
                                <td>Logs fatales</td>
                                <td><IndicadorEstado ok={!datos.fatal_log_detected} /></td>
                            </tr>
                        </tbody>
                    </table>

                    {datos.details.length > 0 && (
                        <div style={{ marginTop: "var(--espacioLg)" }}>
                            <h3 className="tarjetaTitulo">Detalles</h3>
                            <div className="textoMono">
                                {datos.details.join("\n")}
                            </div>
                        </div>
                    )}
                </div>
            )}
        </div>
    );
}

function IndicadorEstado({ ok }: { ok: boolean }) {
    return (
        <span className={`badge ${ok ? "badgeExito" : "badgeError"}`}>
            {ok ? "OK" : "FALLO"}
        </span>
    );
}
