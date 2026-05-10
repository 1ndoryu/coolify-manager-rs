import { useMemo, useState } from "react";
import { usePanelSitios } from "./usePanelSitios";
import type { ResultadoOperacion } from "../tipos";

export function useVistaSitios() {
    const panel = usePanelSitios();
    const [modalAgregarAbierto, setModalAgregarAbierto] = useState(false);
    const conteos = useMemo(() => ({
        online: panel.sitios.filter((sitio) => panel.estados[sitio.name]?.estado === "online").length,
        issues: panel.sitios.filter((sitio) => panel.estados[sitio.name]?.estado === "offline").length,
    }), [panel.estados, panel.sitios]);

    function confirmarSitioCreado(resultado: ResultadoOperacion) {
        panel.registrarOperacion({
            tipo: resultado.success ? "ok" : "error",
            mensaje: resultado.message,
            detalle: resultado.details,
        });
        void panel.cargarSitios();
    }

    return {
        panel,
        conteos,
        modalAgregarAbierto,
        abrirModalAgregar: () => setModalAgregarAbierto(true),
        cerrarModalAgregar: () => setModalAgregarAbierto(false),
        confirmarSitioCreado,
    };
}