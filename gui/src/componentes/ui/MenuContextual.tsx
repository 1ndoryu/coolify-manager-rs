import { MoreHorizontal } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import "./ContextMenu.css";

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
    const botonRef = useRef<HTMLButtonElement>(null);
    const panelRef = useRef<HTMLDivElement>(null);
    const [abierto, setAbierto] = useState(false);
    const [posicion, setPosicion] = useState({ top: 0, left: 0 });

    function abrirMenu() {
        const rect = botonRef.current?.getBoundingClientRect();
        if (!rect) return;
        const anchoPanel = 196;
        setPosicion({
            top: Math.min(rect.bottom + 6, window.innerHeight - 8),
            left: Math.max(8, Math.min(rect.right - anchoPanel, window.innerWidth - anchoPanel - 8)),
        });
        setAbierto((actual) => !actual);
    }

    useEffect(() => {
        if (!abierto) return;

        function cerrarSiClickFuera(event: MouseEvent) {
            const destino = event.target as Node;
            if (botonRef.current?.contains(destino) || panelRef.current?.contains(destino)) return;
            setAbierto(false);
        }

        function cerrarConEscape(event: KeyboardEvent) {
            if (event.key === "Escape") setAbierto(false);
        }

        function cerrarMenu() {
            setAbierto(false);
        }

        window.addEventListener("mousedown", cerrarSiClickFuera);
        window.addEventListener("keydown", cerrarConEscape);
        window.addEventListener("scroll", cerrarMenu, true);
        window.addEventListener("resize", cerrarMenu);
        return () => {
            window.removeEventListener("mousedown", cerrarSiClickFuera);
            window.removeEventListener("keydown", cerrarConEscape);
            window.removeEventListener("scroll", cerrarMenu, true);
            window.removeEventListener("resize", cerrarMenu);
        };
    }, [abierto]);

    return (
        <span className="menuContextual">
            <button ref={botonRef} className="boton botonIcono" type="button" title={etiqueta} aria-label={etiqueta} aria-expanded={abierto} onClick={abrirMenu}>
                <MoreHorizontal size={14} />
            </button>
            {abierto && createPortal(
                <div ref={panelRef} className="menuContextualPanel" role="menu" style={{ top: posicion.top, left: posicion.left }}>
                    {acciones.map((accion) => (
                        <button
                            key={accion.etiqueta}
                            className={`menuContextualItem ${accion.tono === "peligro" ? "menuContextualItemDanger" : ""}`.trim()}
                            type="button"
                            role="menuitem"
                            onClick={() => {
                                setAbierto(false);
                                accion.onClick();
                            }}
                        >
                            {accion.icono && <span className="iconoMenu">{accion.icono}</span>}
                            <span>{accion.etiqueta}</span>
                        </button>
                    ))}
                </div>,
                document.body,
            )}
        </span>
    );
}