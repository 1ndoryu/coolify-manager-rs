/*
 * BarraLateral — navegacion principal compacta.
 */

import { Bot, Boxes, Building2, HardDrive, Settings2, Workflow } from "lucide-react";

const ENLACES = [
    { etiqueta: "Sitios", icono: Building2, activo: true },
    { etiqueta: "Backups", icono: HardDrive, activo: false },
    { etiqueta: "Operaciones", icono: Workflow, activo: false },
    { etiqueta: "Inventario", icono: Boxes, activo: false },
    { etiqueta: "Ajustes", icono: Settings2, activo: false },
];

export function BarraLateral() {
    return (
        <aside className="barraLateral">
            <div className="logoSidebar">
                <span className="marcaIcono"><Bot size={14} strokeWidth={2} /></span>
                <span>Coolify</span>
            </div>
            <nav className="navegacionSidebar">
                {ENLACES.map(({ etiqueta, icono: Icono, activo }) => (
                    <button
                        key={etiqueta}
                        className={`enlaceNav ${activo ? "enlaceNavActivo" : ""}`}
                        disabled={!activo}
                        title={activo ? etiqueta : `${etiqueta} se integra en fases siguientes`}
                    >
                        <span className="iconoNav"><Icono size={14} strokeWidth={1.8} /></span>
                        {etiqueta}
                    </button>
                ))}
            </nav>
        </aside>
    );
}
