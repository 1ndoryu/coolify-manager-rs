/*
 * App — componente raiz.
 * Maneja la vista activa y el layout sidebar + contenido.
 */

import { useState } from "react";
import { BarraLateral } from "./componentes/BarraLateral";
import { VistaSitios } from "./componentes/VistaSitios";
import { VistaSalud } from "./componentes/VistaSalud";
import { VistaBackups } from "./componentes/VistaBackups";
import { VistaAuditoria } from "./componentes/VistaAuditoria";
import type { Vista } from "./tipos";
import "./estilos/layout.css";
import "./estilos/componentes.css";

export function App() {
    const [vistaActiva, setVistaActiva] = useState<Vista>("sitios");
    const [sitioSeleccionado, setSitioSeleccionado] = useState<string | null>(null);

    const renderizarVista = () => {
        switch (vistaActiva) {
            case "sitios":
                return <VistaSitios onSeleccionar={(nombre) => {
                    setSitioSeleccionado(nombre);
                    setVistaActiva("salud");
                }} />;
            case "salud":
                return <VistaSalud sitioInicial={sitioSeleccionado} />;
            case "backups":
                return <VistaBackups sitioInicial={sitioSeleccionado} />;
            case "auditoria":
                return <VistaAuditoria />;
        }
    };

    return (
        <div className="contenedorLayout">
            <BarraLateral vistaActiva={vistaActiva} onCambiarVista={setVistaActiva} />
            <main className="contenidoPrincipal">
                {renderizarVista()}
            </main>
        </div>
    );
}
