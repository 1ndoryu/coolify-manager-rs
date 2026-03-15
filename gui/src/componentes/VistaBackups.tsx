/*
 * VistaBackups — listar backups de un sitio.
 */

import { useState } from "react";
import { useComandoTauri } from "../hooks/useComandoTauri";
import type { RespuestaBackups } from "../tipos";

interface Props {
    sitioInicial: string | null;
}

export function VistaBackups({ sitioInicial }: Props) {
    const [sitio, setSitio] = useState(sitioInicial ?? "");
    const { datos, cargando, error, ejecutar } = useComandoTauri<RespuestaBackups>("list_backups");

    const listar = () => {
        if (!sitio.trim()) return;
        ejecutar({ siteName: sitio.trim() });
    };

    return (
        <div>
            <div className="cabeceraPagina">
                <h1 className="tituloPagina">Backups</h1>
                <p className="subtituloPagina">Copias de seguridad disponibles</p>
            </div>

            <div className="tarjeta" style={{ marginBottom: "var(--espacioLg)" }}>
                <div style={{ display: "flex", gap: "var(--espacioSm)", alignItems: "center" }}>
                    <input
                        type="text"
                        value={sitio}
                        onChange={(e) => setSitio(e.target.value)}
                        onKeyDown={(e) => e.key === "Enter" && listar()}
                        placeholder="Nombre del sitio"
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
                        onClick={listar}
                    >
                        {cargando ? "Listando..." : "Listar backups"}
                    </button>
                </div>
            </div>

            {error && <div className="mensajeError">{error}</div>}

            {datos && (
                <div className="contenedorTabla">
                    {datos.backups.length === 0 ? (
                        <div className="estadoVacio">No hay backups para {datos.site_name}</div>
                    ) : (
                        <table className="tabla">
                            <thead>
                                <tr>
                                    <th>ID</th>
                                    <th>Tier</th>
                                    <th>Estado</th>
                                    <th>Fecha</th>
                                    <th>Label</th>
                                    <th>Artefactos</th>
                                </tr>
                            </thead>
                            <tbody>
                                {datos.backups.map((b) => (
                                    <tr key={b.backup_id}>
                                        <td className="textoMono">{b.backup_id}</td>
                                        <td>
                                            <span className="badge badgeInfo">{b.tier}</span>
                                        </td>
                                        <td>
                                            <span className={`badge ${b.status === "Ready" ? "badgeExito" : b.status === "Failed" ? "badgeError" : "badgeAdvertencia"}`}>
                                                {b.status}
                                            </span>
                                        </td>
                                        <td>{new Date(b.created_at).toLocaleString("es-ES")}</td>
                                        <td>{b.label ?? "-"}</td>
                                        <td>{b.artifact_count}</td>
                                    </tr>
                                ))}
                            </tbody>
                        </table>
                    )}
                </div>
            )}
        </div>
    );
}
