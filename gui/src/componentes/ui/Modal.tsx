import { X } from "lucide-react";
import { useEffect, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { IconButton } from "./Button";
import "./Modal.css";

interface ModalProps {
    abierto: boolean;
    titulo: string;
    children: ReactNode;
    acciones?: ReactNode;
    onCerrar: () => void;
}

export function Modal({ abierto, titulo, children, acciones, onCerrar }: ModalProps) {
    useEffect(() => {
        if (!abierto) return;

        function cerrarConEscape(event: KeyboardEvent) {
            if (event.key === "Escape") onCerrar();
        }

        window.addEventListener("keydown", cerrarConEscape);
        return () => window.removeEventListener("keydown", cerrarConEscape);
    }, [abierto, onCerrar]);

    if (!abierto) return null;

    return createPortal(
        <div className="modalFondo" role="presentation" onMouseDown={onCerrar}>
            <section className="modalPanel" role="dialog" aria-modal="true" aria-labelledby="modalTitulo" onMouseDown={(event) => event.stopPropagation()}>
                <header className="modalCabecera">
                    <h2 id="modalTitulo" className="modalTitulo">{titulo}</h2>
                    <IconButton icon={<X size={14} />} type="button" title="Cerrar" aria-label="Cerrar" onClick={onCerrar} />
                </header>
                <div className="modalCuerpo">{children}</div>
                {acciones && <footer className="modalAcciones">{acciones}</footer>}
            </section>
        </div>,
        document.body,
    );
}