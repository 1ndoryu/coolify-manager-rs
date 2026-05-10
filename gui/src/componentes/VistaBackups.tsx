import { Archive, RefreshCw } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { ejecutarComandoGui, type ModoCliente } from "../servicios/clienteCoolify";
import type { RespuestaBackups, RespuestaSitios, ResumenBackup, SitioResumen } from "../tipos";
import { Button } from "./ui/Button";

export function VistaBackups() {
    const [sitios, setSitios] = useState<SitioResumen[]>([]);
    const [sitioActivo, setSitioActivo] = useState("");
    const [backups, setBackups] = useState<ResumenBackup[]>([]);
    const [modoCliente, setModoCliente] = useState<ModoCliente>("navegador");
    const [cargando, setCargando] = useState(true);
    const [error, setError] = useState<string | null>(null);

    const sitioSeleccionado = useMemo(() => sitios.find((sitio) => sitio.name === sitioActivo), [sitioActivo, sitios]);

    async function cargarSitios() {
        const resultado = await ejecutarComandoGui<RespuestaSitios>("list_sites");
        setModoCliente(resultado.modo);
        setSitios(resultado.datos.sites);
        setSitioActivo((actual) => actual || resultado.datos.sites[0]?.name || "");
    }

    async function cargarBackups(siteName: string) {
        if (!siteName) return;
        setCargando(true);
        setError(null);
        try {
            const resultado = await ejecutarComandoGui<RespuestaBackups>("list_backups", { siteName });
            setModoCliente(resultado.modo);
            setBackups(resultado.datos.backups);
        } catch (err) {
            setError(err instanceof Error ? err.message : String(err));
            setBackups([]);
        } finally {
            setCargando(false);
        }
    }

    useEffect(() => {
        void cargarSitios();
    }, []);

    useEffect(() => {
        void cargarBackups(sitioActivo);
    }, [sitioActivo]);

    return (
        <div className="vistaConsola">
            <header className="barraSuperior">
                <div>
                    <div className="rutaPagina">Coolify / Copias</div>
                    <h1 className="tituloPagina">Copias por sitio</h1>
                </div>
                <div className="accionesSuperiores">
                    <span className={`badge ${modoCliente === "tauri" ? "badgeExito" : "badgeNeutro"}`}>{modoCliente === "tauri" ? "Modo real" : "Modo navegador"}</span>
                    <select className="selectorCompacto" value={sitioActivo} onChange={(event) => setSitioActivo(event.target.value)}>
                        {sitios.length === 0 && <option value="">Cargando sitios...</option>}
                        {sitios.map((sitio) => <option key={sitio.name} value={sitio.name}>{sitio.name}</option>)}
                    </select>
                    <Button onClick={() => void cargarBackups(sitioActivo)}><RefreshCw size={14} /> Actualizar</Button>
                </div>
            </header>

            {error && <div className="mensajeError">{error}</div>}

            <section className="panelTabla">
                <div className="toolbarTabla">
                    <div className="grupoToolbar"><Archive size={14} /> {sitioSeleccionado?.domain ?? "Selecciona un sitio"}</div>
                    <span>{backups.length} copias</span>
                </div>
                {cargando ? (
                    <div className="cargando bloquePanel"><div className="spinner" /> Cargando copias...</div>
                ) : backups.length === 0 ? (
                    <div className="estadoVacio">No hay copias registradas para este sitio.</div>
                ) : (
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
                )}
            </section>
        </div>
    );
}