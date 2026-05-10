/*
 * BarraLateral — navegacion principal compacta.
 */

import { Bot, Building2, Gauge, HardDrive, Settings2 } from "lucide-react";
import type { VistaPrincipal } from "../App";
import type { ModoCliente } from "../servicios/clienteCoolify";
import type { TargetResumen } from "../tipos";
import { SelectorVps } from "./SelectorVps";

const ENLACES = [
    { id: "dashboard", etiqueta: "Panel", icono: Gauge },
    { id: "sitios", etiqueta: "Sitios", icono: Building2 },
    { id: "backups", etiqueta: "Copias", icono: HardDrive },
    { id: "ajustes", etiqueta: "Ajustes", icono: Settings2 },
];

interface BarraLateralProps {
    vistaActiva: VistaPrincipal;
    targets: TargetResumen[];
    targetActivo: string;
    modoCliente: ModoCliente;
    cargandoTargets: boolean;
    errorTargets: string | null;
    onCambiarVista: (vista: VistaPrincipal) => void;
    onCambiarTarget: (target: string) => void;
    onActualizarTargets: () => void;
}

export function BarraLateral({ vistaActiva, targets, targetActivo, modoCliente, cargandoTargets, errorTargets, onCambiarVista, onCambiarTarget, onActualizarTargets }: BarraLateralProps) {
    return (
        <aside className="barraLateral">
            <div className="logoSidebar">
                <div className="marcaSidebar">
                    <span className="marcaIcono"><Bot size={14} strokeWidth={2} /></span>
                    <span>Coolify</span>
                </div>
                <SelectorVps
                    targets={targets}
                    targetActivo={targetActivo}
                    modoCliente={modoCliente}
                    cargando={cargandoTargets}
                    error={errorTargets}
                    onCambiarTarget={onCambiarTarget}
                    onActualizarTargets={onActualizarTargets}
                />
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
