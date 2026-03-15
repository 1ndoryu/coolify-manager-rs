/*
 * VistaAuditoria — auditoria de VPS.
 */

import { useComandoTauri } from "../hooks/useComandoTauri";
import type { RespuestaAuditoria } from "../tipos";

export function VistaAuditoria() {
    const { datos, cargando, error, ejecutar } = useComandoTauri<RespuestaAuditoria>("audit_vps");

    const auditar = () => { ejecutar({}); };

    return (
        <div>
            <div className="cabeceraPagina">
                <h1 className="tituloPagina">Auditoria VPS</h1>
                <p className="subtituloPagina">Estado del servidor principal</p>
            </div>

            <div style={{ marginBottom: "var(--espacioLg)" }}>
                <button
                    className={`boton botonPrimario ${cargando ? "botonDeshabilitado" : ""}`}
                    onClick={auditar}
                >
                    {cargando ? "Auditando..." : "Ejecutar auditoria"}
                </button>
            </div>

            {error && <div className="mensajeError">{error}</div>}

            {datos && (
                <div className="gridTarjetas">
                    <TarjetaMetrica titulo="Target" valor={datos.target} />
                    <TarjetaMetrica titulo="Carga" valor={datos.load_average} />
                    <TarjetaMetrica titulo="Memoria" valor={datos.memory_summary} />
                    <TarjetaMetrica titulo="Disco" valor={datos.disk_summary} />
                    <TarjetaMetrica titulo="Docker" valor={datos.docker_summary} />
                    <TarjetaMetrica titulo="Seguridad" valor={datos.security_summary} />

                    {datos.recommendations.length > 0 && (
                        <div className="tarjeta" style={{ gridColumn: "1 / -1" }}>
                            <h3 className="tarjetaTitulo">Recomendaciones</h3>
                            <ul style={{ paddingLeft: "var(--espacioLg)", color: "var(--advertencia)" }}>
                                {datos.recommendations.map((rec, i) => (
                                    <li key={i} style={{ marginBottom: "var(--espacioXs)" }}>{rec}</li>
                                ))}
                            </ul>
                        </div>
                    )}
                </div>
            )}
        </div>
    );
}

function TarjetaMetrica({ titulo, valor }: { titulo: string; valor: string }) {
    return (
        <div className="tarjeta">
            <div style={{ color: "var(--textoSecundario)", fontSize: "var(--fuenteSm)", marginBottom: "var(--espacioXs)" }}>
                {titulo}
            </div>
            <div className="textoMono" style={{ fontSize: "var(--fuenteSm)" }}>
                {valor}
            </div>
        </div>
    );
}
