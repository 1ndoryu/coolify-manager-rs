import { Check, ChevronDown } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import "./ContextMenu.css";
import "./SelectorPersonalizado.css";

export interface OpcionSelector {
    valor: string;
    etiqueta: string;
    detalle?: string;
}

interface SelectorPersonalizadoProps {
    etiqueta: string;
    valor: string;
    opciones: OpcionSelector[];
    placeholder?: string;
    onCambiar: (valor: string) => void;
}

export function SelectorPersonalizado({ etiqueta, valor, opciones, placeholder = "Seleccionar", onCambiar }: SelectorPersonalizadoProps) {
    const contenedorRef = useRef<HTMLDivElement>(null);
    const [abierto, setAbierto] = useState(false);
    const opcionActiva = opciones.find((opcion) => opcion.valor === valor);

    useEffect(() => {
        if (!abierto) return;

        function cerrarSiClickFuera(event: MouseEvent) {
            const destino = event.target as Node;
            if (contenedorRef.current?.contains(destino)) return;
            setAbierto(false);
        }

        function cerrarConEscape(event: KeyboardEvent) {
            if (event.key === "Escape") setAbierto(false);
        }

        window.addEventListener("mousedown", cerrarSiClickFuera);
        window.addEventListener("keydown", cerrarConEscape);
        return () => {
            window.removeEventListener("mousedown", cerrarSiClickFuera);
            window.removeEventListener("keydown", cerrarConEscape);
        };
    }, [abierto]);

    return (
        <div className="selectorPersonalizado" ref={contenedorRef}>
            <button
                className="selectorPersonalizadoBoton"
                type="button"
                aria-label={etiqueta}
                aria-haspopup="listbox"
                aria-expanded={abierto}
                onClick={() => setAbierto((actual) => !actual)}
            >
                <span className="selectorPersonalizadoTexto">
                    <span>{opcionActiva?.etiqueta ?? placeholder}</span>
                    {opcionActiva?.detalle && <small>{opcionActiva.detalle}</small>}
                </span>
                <ChevronDown size={14} />
            </button>
            {abierto && (
                <div className="selectorPersonalizadoPanel menuContextualPanel" role="listbox" aria-label={etiqueta}>
                    {opciones.map((opcion) => (
                        <button
                            key={opcion.valor}
                            className="selectorPersonalizadoOpcion menuContextualItem"
                            type="button"
                            role="option"
                            aria-selected={opcion.valor === valor}
                            onClick={() => {
                                onCambiar(opcion.valor);
                                setAbierto(false);
                            }}
                        >
                            <span className="selectorPersonalizadoTexto">
                                <span>{opcion.etiqueta}</span>
                                {opcion.detalle && <small>{opcion.detalle}</small>}
                            </span>
                            {opcion.valor === valor && <Check size={14} />}
                        </button>
                    ))}
                    {opciones.length === 0 && <div className="selectorPersonalizadoVacio">Sin opciones</div>}
                </div>
            )}
        </div>
    );
}