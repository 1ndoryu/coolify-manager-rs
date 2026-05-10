import { MoreHorizontal } from "lucide-react";
import type { ReactNode } from "react";

export interface AccionMenu {
    etiqueta: string;
    icono?: ReactNode;
    tono?: "normal" | "peligro";
    onClick: () => void;
}

interface MenuContextualProps {
    etiqueta: string;
    acciones: AccionMenu[];
}

export function MenuContextual({ etiqueta, acciones }: MenuContextualProps) {
    return (
        <details className="menuContextual">
            <summary className="boton botonIcono resumenMenu" title={etiqueta} aria-label={etiqueta}>
                <MoreHorizontal size={14} />
            </summary>
            <div className="panelMenu" role="menu">
                {acciones.map((accion) => (
                    <button
                        key={accion.etiqueta}
                        className={`itemMenu ${accion.tono === "peligro" ? "itemMenuPeligro" : ""}`.trim()}
                        type="button"
                        role="menuitem"
                        onClick={(event) => {
                            event.currentTarget.closest("details")?.removeAttribute("open");
                            accion.onClick();
                        }}
                    >
                        {accion.icono && <span className="iconoMenu">{accion.icono}</span>}
                        <span>{accion.etiqueta}</span>
                    </button>
                ))}
            </div>
        </details>
    );
}