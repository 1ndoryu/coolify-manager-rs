/*
 * App — consola operativa de Coolify Manager.
 * [125A-3] Guard de autenticación: si !autenticado → VistaLogin en lugar del layout operativo.
 */

import { BarraLateral } from "./componentes/BarraLateral";
import { VistaAjustes } from "./componentes/VistaAjustes";
import { VistaBackups } from "./componentes/VistaBackups";
import { VistaDashboard } from "./componentes/VistaDashboard";
import { VistaLogin } from "./componentes/VistaLogin";
import { VistaSitios } from "./componentes/VistaSitios";
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
    const targetsGlobales = useGlobalTargets();

    if (auth.cargando) {
        return <div className="contenedorCarga"><span>Iniciando…</span></div>;
    }

    if (!auth.autenticado) {
        return <VistaLogin onLogin={auth.login} error={auth.error} />;
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

