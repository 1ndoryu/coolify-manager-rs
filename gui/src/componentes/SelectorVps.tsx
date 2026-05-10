import { RefreshCw, Server } from "lucide-react";
import { claseModoCliente, etiquetaModoCliente, type ModoCliente } from "../servicios/clienteCoolify";
import type { TargetResumen } from "../tipos";
import { IconButton } from "./ui/Button";
import { SelectorPersonalizado } from "./ui/SelectorPersonalizado";

interface SelectorVpsProps {
    targets: TargetResumen[];
    targetActivo: string;
    modoCliente: ModoCliente;
    cargando: boolean;
    error: string | null;
    onCambiarTarget: (target: string) => void;
    onActualizarTargets: () => void;
}

export function SelectorVps({ targets, targetActivo, modoCliente, cargando, error, onCambiarTarget, onActualizarTargets }: SelectorVpsProps) {
    const opciones = targets.map((target) => ({
        valor: target.name,
        etiqueta: target.name,
        detalle: `${target.host} · ${target.site_count} sitios`,
    }));

    return (
        <div className="selectorVpsSidebar">
            <div className="selectorVpsCabecera">
                <span><Server size={13} /> VPS</span>
                <IconButton icon={<RefreshCw size={13} />} type="button" title="Actualizar VPS" aria-label="Actualizar VPS" onClick={onActualizarTargets} />
            </div>
            <SelectorPersonalizado
                etiqueta="Cambiar VPS activo"
                valor={targetActivo}
                opciones={opciones}
                placeholder={cargando ? "Cargando VPS..." : "Sin VPS"}
                onCambiar={onCambiarTarget}
            />
            <div className="selectorVpsEstado">
                <span className={`badge ${claseModoCliente(modoCliente)}`}>{etiquetaModoCliente(modoCliente)}</span>
                {error && <span className="selectorVpsError" title={error}>Error al leer VPS</span>}
            </div>
        </div>
    );
}