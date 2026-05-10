/*
 * BarraLateral — navegacion principal compacta.
 */

import { Bot, Building2, Gauge, HardDrive, Settings2 } from "lucide-react";
import type { VistaPrincipal } from "../App";

const ENLACES = [
    { id: "dashboard", etiqueta: "Panel", icono: Gauge },
    { id: "sitios", etiqueta: "Sitios", icono: Building2 },
    { id: "backups", etiqueta: "Copias", icono: HardDrive },
    { id: "ajustes", etiqueta: "Ajustes", icono: Settings2 },
];

interface BarraLateralProps {
    vistaActiva: VistaPrincipal;
    onCambiarVista: (vista: VistaPrincipal) => void;
}

export function BarraLateral({ vistaActiva, onCambiarVista }: BarraLateralProps) {
    return (
        <aside className="barraLateral">
            <div className="logoSidebar">
                <span className="marcaIcono"><Bot size={14} strokeWidth={2} /></span>
                <span>Coolify</span>
            </div>
            <nav className="navegacionSidebar">
                {ENLACES.map(({ id, etiqueta, icono: Icono }) => (
                    <button
                        key={etiqueta}
                        className={`enlaceNav ${vistaActiva === id ? "enlaceNavActivo" : ""}`}
                        onClick={() => onCambiarVista(id as VistaPrincipal)}
                        title={etiqueta}
                    >
                        <span className="iconoNav"><Icono size={14} strokeWidth={1.8} /></span>
                        {etiqueta}
                    </button>
                ))}
            </nav>
        </aside>
    );
}
