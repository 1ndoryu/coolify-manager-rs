/*
 * App — consola operativa de Coolify Manager.
 */

import { BarraLateral } from "./componentes/BarraLateral";
import { VistaAjustes } from "./componentes/VistaAjustes";
import { VistaBackups } from "./componentes/VistaBackups";
import { VistaDashboard } from "./componentes/VistaDashboard";
import { VistaSitios } from "./componentes/VistaSitios";
import { useState } from "react";
import { useGlobalTargets } from "./hooks/useGlobalTargets";
import "./estilos/layout.css";
import "./estilos/componentes.css";

export type VistaPrincipal = "dashboard" | "sitios" | "backups" | "ajustes";

export function App() {
    const [vistaActiva, setVistaActiva] = useState<VistaPrincipal>("dashboard");
    const [filtroCopias, setFiltroCopias] = useState("");
    const targetsGlobales = useGlobalTargets();

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
