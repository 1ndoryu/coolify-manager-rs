import { OPCIONES_TEMPLATE, useModalAgregarSitio } from "../hooks/useModalAgregarSitio";
import type { ResultadoOperacion, TargetResumen } from "../tipos";
import { Button } from "./ui/Button";
import { Modal } from "./ui/Modal";
import { SelectorPersonalizado } from "./ui/SelectorPersonalizado";

interface ModalAgregarSitioProps {
    abierto: boolean;
    targets: TargetResumen[];
    targetActivo: string;
    onCerrar: () => void;
    onCreado: (resultado: ResultadoOperacion) => void;
}

export function ModalAgregarSitio({ abierto, targets, targetActivo, onCerrar, onCreado }: ModalAgregarSitioProps) {
    const { borrador, enviando, error, opcionesTargets, actualizarBorrador, crearSitio } = useModalAgregarSitio({ abierto, targets, targetActivo, onCerrar, onCreado });

    return (
        <Modal
            abierto={abierto}
            titulo="Agregar sitio"
            onCerrar={onCerrar}
            acciones={(
                <>
                    <Button type="button" onClick={onCerrar}>Cancelar</Button>
                    <Button variant="primario" type="button" disabled={enviando} onClick={() => void crearSitio()}>
                        {enviando ? "Creando..." : "Crear sitio"}
                    </Button>
                </>
            )}
        >
            <div className="modalFormulario">
                {error && <div className="mensajeError">{error}</div>}
                <label>Nombre<input className="campoTexto" value={borrador.name} onChange={(event) => actualizarBorrador("name", event.target.value)} placeholder="mi-sitio" /></label>
                <label>Dominio<input className="campoTexto" value={borrador.domain} onChange={(event) => actualizarBorrador("domain", event.target.value)} placeholder="https://mi-sitio.com" /></label>
                <div><span>Plantilla</span><SelectorPersonalizado etiqueta="Plantilla del sitio" valor={borrador.template} opciones={OPCIONES_TEMPLATE} onCambiar={(valor) => actualizarBorrador("template", valor)} /></div>
                <div><span>VPS</span><SelectorPersonalizado etiqueta="VPS destino" valor={borrador.target} opciones={opcionesTargets} placeholder="Sin VPS" onCambiar={(valor) => actualizarBorrador("target", valor)} /></div>
            </div>
        </Modal>
    );
}