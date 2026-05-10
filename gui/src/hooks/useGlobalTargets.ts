import { useEffect, useState } from "react";
import { ejecutarComandoGui, type ModoCliente } from "../servicios/clienteCoolify";
import type { RespuestaTargets, TargetResumen } from "../tipos";

export function useGlobalTargets() {
    const [targets, setTargets] = useState<TargetResumen[]>([]);
    const [targetActivo, setTargetActivo] = useState("default");
    const [configPath, setConfigPath] = useState("--");
    const [modoCliente, setModoCliente] = useState<ModoCliente>("local");
    const [cargandoTargets, setCargandoTargets] = useState(true);
    const [errorTargets, setErrorTargets] = useState<string | null>(null);

    /* [105A-29] La carga de targets queda aislada para que App solo coordine vistas
     * y el selector global del sidebar no replique estado en Dashboard/Ajustes. */
    async function cargarTargets(force = false) {
        setCargandoTargets(true);
        setErrorTargets(null);
        try {
            const resultado = await ejecutarComandoGui<RespuestaTargets>("list_targets", { force });
            const siguiente = resultado.datos.default_target || resultado.datos.targets[0]?.name || "default";
            setModoCliente(resultado.modo);
            setTargets(resultado.datos.targets);
            setConfigPath(resultado.datos.config_path);
            setTargetActivo((actual) => resultado.datos.targets.some((target) => target.name === actual) ? actual : siguiente);
        } catch (err) {
            setErrorTargets(err instanceof Error ? err.message : String(err));
        } finally {
            setCargandoTargets(false);
        }
    }

    useEffect(() => {
        void cargarTargets();
    }, []);

    return {
        targets,
        targetActivo,
        configPath,
        modoCliente,
        cargandoTargets,
        errorTargets,
        setTargetActivo,
        cargarTargets,
    };
}