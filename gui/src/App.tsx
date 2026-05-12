/*
 * App — consola operativa de Coolify Manager.
 * [125A-3] Guard de autenticación: si !autenticado → VistaPortal (landing) con modal login.
 * [125A-5] Landing page pública para vps.nakomi.studio. Login como modal overlay.
 */

import { BarraLateral } from "./componentes/BarraLateral";
import { VistaAjustes } from "./componentes/VistaAjustes";
import { VistaBackups } from "./componentes/VistaBackups";
import { VistaDashboard } from "./componentes/VistaDashboard";
import { VistaPortal } from "./componentes/VistaPortal";
import { VistaSitios } from "./componentes/VistaSitios";
import { Modal } from "./componentes/ui/Modal";
import { useAuth } from "./hooks/useAuth";
import { useState } from "react";
import { useGlobalTargets } from "./hooks/useGlobalTargets";
import "./estilos/layout.css";
import "./estilos/componentes.css";
import "./estilos/login.css";

export type VistaPrincipal = "dashboard" | "sitios" | "backups" | "ajustes";

export function App() {
    const auth = useAuth();
    const [vistaActiva, setVistaActiva] = useState<VistaPrincipal>("dashboard");
    const [filtroCopias, setFiltroCopias] = useState("");
    const [modalLoginAbierto, setModalLoginAbierto] = useState(false);
    const targetsGlobales = useGlobalTargets();

    if (auth.cargando) {
        return <div className="contenedorCarga"><span>Iniciando…</span></div>;
    }

    if (!auth.autenticado) {
        return <PortalConLogin auth={auth} modalAbierto={modalLoginAbierto} setModalAbierto={setModalLoginAbierto} />;
    }

    function abrirCopiasDeSitio(siteName: string) {
        setFiltroCopias(siteName);
        setVistaActiva("backups");
    }

    return (
        <div className="contenedorLayout">
            <BarraLateral
                vistaActiva={vistaActiva}
                targets={targetsGlobales.targets}
                targetActivo={targetsGlobales.targetActivo}
                modoCliente={targetsGlobales.modoCliente}
                cargandoTargets={targetsGlobales.cargandoTargets}
                errorTargets={targetsGlobales.errorTargets}
                onCambiarVista={setVistaActiva}
                onCambiarTarget={targetsGlobales.setTargetActivo}
                onActualizarTargets={() => void targetsGlobales.cargarTargets(true)}
            />
            <main className="contenidoPrincipal">
                {vistaActiva === "dashboard" && <VistaDashboard targets={targetsGlobales.targets} targetActivo={targetsGlobales.targetActivo} modoCliente={targetsGlobales.modoCliente} onCambiarTarget={targetsGlobales.setTargetActivo} />}
                {vistaActiva === "sitios" && <VistaSitios targets={targetsGlobales.targets} targetActivo={targetsGlobales.targetActivo} onVerCopiasSitio={abrirCopiasDeSitio} />}
                {vistaActiva === "backups" && <VistaBackups filtroInicial={filtroCopias} onCambiarFiltro={setFiltroCopias} />}
                {vistaActiva === "ajustes" && <VistaAjustes targets={targetsGlobales.targets} targetActivo={targetsGlobales.targetActivo} configPath={targetsGlobales.configPath} modoCliente={targetsGlobales.modoCliente} />}
            </main>
        </div>
    );
}

/* [125A-5] Portal con modal de login inline.
 * Separado en componente propio para que el hook useState del modal
 * no afecte el render del layout operativo autenticado. */
interface PortalConLoginProps {
    auth: ReturnType<typeof useAuth>;
    modalAbierto: boolean;
    setModalAbierto: (v: boolean) => void;
}

function PortalConLogin({ auth, modalAbierto, setModalAbierto }: PortalConLoginProps) {
    const [email, setEmail] = useState("");
    const [password, setPassword] = useState("");
    const [cargando, setCargando] = useState(false);

    async function handleSubmit(e: React.FormEvent) {
        e.preventDefault();
        setCargando(true);
        await auth.login(email, password);
        setCargando(false);
    }

    return (
        <>
            <VistaPortal onAbrirLogin={() => setModalAbierto(true)} />
            <Modal
                abierto={modalAbierto}
                titulo="Acceder al panel"
                onCerrar={() => setModalAbierto(false)}
            >
                <form onSubmit={handleSubmit} className="formularioLogin">
                    <div className="campoLogin">
                        <label htmlFor="loginEmail">Correo</label>
                        <input
                            id="loginEmail"
                            type="email"
                            value={email}
                            onChange={e => setEmail(e.target.value)}
                            required
                            autoComplete="email"
                            disabled={cargando}
                        />
                    </div>
                    <div className="campoLogin">
                        <label htmlFor="loginPassword">Contraseña</label>
                        <input
                            id="loginPassword"
                            type="password"
                            value={password}
                            onChange={e => setPassword(e.target.value)}
                            required
                            autoComplete="current-password"
                            disabled={cargando}
                        />
                    </div>
                    {auth.error && <p className="errorLogin">{auth.error}</p>}
                    <button type="submit" className="botonLogin" disabled={cargando || !email || !password}>
                        {cargando ? "Verificando…" : "Entrar"}
                    </button>
                </form>
            </Modal>
        </>
    );
}
