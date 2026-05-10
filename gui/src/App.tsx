/*
 * App — consola operativa de Coolify Manager.
 */

import { BarraLateral } from "./componentes/BarraLateral";
import { VistaAjustes } from "./componentes/VistaAjustes";
import { VistaBackups } from "./componentes/VistaBackups";
import { VistaDashboard } from "./componentes/VistaDashboard";
import { VistaSitios } from "./componentes/VistaSitios";
import { useState } from "react";
import "./estilos/layout.css";
import "./estilos/componentes.css";

export type VistaPrincipal = "dashboard" | "sitios" | "backups" | "ajustes";

export function App() {
    const [vistaActiva, setVistaActiva] = useState<VistaPrincipal>("dashboard");

    return (
        <div className="contenedorLayout">
            <BarraLateral vistaActiva={vistaActiva} onCambiarVista={setVistaActiva} />
            <main className="contenidoPrincipal">
                {vistaActiva === "dashboard" && <VistaDashboard />}
                {vistaActiva === "sitios" && <VistaSitios onAgregarSitio={() => setVistaActiva("ajustes")} />}
                {vistaActiva === "backups" && <VistaBackups />}
                {vistaActiva === "ajustes" && <VistaAjustes />}
            </main>
        </div>
    );
}
