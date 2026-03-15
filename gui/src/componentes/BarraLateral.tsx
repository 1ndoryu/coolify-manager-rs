/*
 * BarraLateral — navegacion principal.
 */

import type { Vista } from "../tipos";

interface Props {
    vistaActiva: Vista;
    onCambiarVista: (vista: Vista) => void;
}

const ENLACES: { vista: Vista; etiqueta: string; icono: string }[] = [
    { vista: "sitios", etiqueta: "Sitios", icono: "S" },
    { vista: "salud", etiqueta: "Salud", icono: "H" },
    { vista: "backups", etiqueta: "Backups", icono: "B" },
    { vista: "auditoria", etiqueta: "Auditoria", icono: "A" },
];

export function BarraLateral({ vistaActiva, onCambiarVista }: Props) {
    return (
        <aside className="barraLateral">
            <div className="logoSidebar">Coolify Manager</div>
            <nav className="navegacionSidebar">
                {ENLACES.map(({ vista, etiqueta, icono }) => (
                    <button
                        key={vista}
                        className={`enlaceNav ${vistaActiva === vista ? "enlaceNavActivo" : ""}`}
                        onClick={() => onCambiarVista(vista)}
                    >
                        <span className="iconoNav">{icono}</span>
                        {etiqueta}
                    </button>
                ))}
            </nav>
        </aside>
    );
}
