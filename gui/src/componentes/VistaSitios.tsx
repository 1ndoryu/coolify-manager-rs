/*
 * VistaSitios — lista de sitios configurados.
 */

import { useEffect } from "react";
import { useComandoTauri } from "../hooks/useComandoTauri";
import type { RespuestaSitios } from "../tipos";

interface Props {
    onSeleccionar: (nombre: string) => void;
}

export function VistaSitios({ onSeleccionar }: Props) {
    const { datos, cargando, error, ejecutar } = useComandoTauri<RespuestaSitios>("list_sites");

    useEffect(() => { ejecutar(); }, [ejecutar]);

    return (
        <div>
            <div className="cabeceraPagina">
                <h1 className="tituloPagina">Sitios</h1>
                <p className="subtituloPagina">
                    {datos ? `${datos.sites.length} sitios configurados` : "Cargando..."}
                </p>
            </div>

            {cargando && (
                <div className="cargando">
                    <div className="spinner" />
                    Cargando sitios...
                </div>
            )}

            {error && <div className="mensajeError">{error}</div>}

            {datos && (
                <div className="contenedorTabla">
                    <table className="tabla">
                        <thead>
                            <tr>
                                <th>Nombre</th>
                                <th>Dominio</th>
                                <th>Target</th>
                                <th>Template</th>
                                <th>Acciones</th>
                            </tr>
                        </thead>
                        <tbody>
                            {datos.sites.map((sitio) => (
                                <tr key={sitio.name}>
                                    <td>{sitio.name}</td>
                                    <td>
                                        <span className="textoMono">{sitio.domain}</span>
                                    </td>
                                    <td>
                                        <span className="badge badgeInfo">{sitio.target}</span>
                                    </td>
                                    <td>{sitio.template}</td>
                                    <td>
                                        <button
                                            className="boton botonSecundario"
                                            onClick={() => onSeleccionar(sitio.name)}
                                        >
                                            Health
                                        </button>
                                    </td>
                                </tr>
                            ))}
                        </tbody>
                    </table>

                    {datos.minecraft.length > 0 && (
                        <>
                            <h2 className="tarjetaTitulo" style={{ marginTop: "var(--espacioXl)" }}>
                                Minecraft
                            </h2>
                            <table className="tabla">
                                <thead>
                                    <tr>
                                        <th>Servidor</th>
                                        <th>Memoria</th>
                                        <th>Max Jugadores</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {datos.minecraft.map((mc) => (
                                        <tr key={mc.name}>
                                            <td>{mc.name}</td>
                                            <td>{mc.memory}</td>
                                            <td>{mc.max_players}</td>
                                        </tr>
                                    ))}
                                </tbody>
                            </table>
                        </>
                    )}
                </div>
            )}
        </div>
    );
}
