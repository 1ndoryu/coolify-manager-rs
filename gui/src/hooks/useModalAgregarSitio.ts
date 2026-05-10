import { useEffect, useMemo, useState } from "react";
import { ejecutarComandoGui } from "../servicios/clienteCoolify";
import type { CrearSitioRequest, ResultadoOperacion, TargetResumen } from "../tipos";

interface UseModalAgregarSitioProps {
    abierto: boolean;
    targets: TargetResumen[];
    targetActivo: string;
    onCerrar: () => void;
    onCreado: (resultado: ResultadoOperacion) => void;
}

export const OPCIONES_TEMPLATE = [
    { valor: "wordpress", etiqueta: "WordPress" },
    { valor: "rust", etiqueta: "Rust" },
    { valor: "kamples", etiqueta: "Kamples" },
];

function borradorInicial(targetActivo: string): CrearSitioRequest {
    return {
        name: "",
        domain: "https://",
        template: "wordpress",
        target: targetActivo,
    };
}

function validarBorrador(borrador: CrearSitioRequest): string | null {
    if (!/^[a-z0-9][a-z0-9-]{1,62}$/.test(borrador.name.trim())) {
        return "El nombre debe ser un slug de 2 a 63 caracteres en minusculas, numeros o guiones.";
    }

    try {
        const url = new URL(borrador.domain.trim());
        if (!/^https?:$/.test(url.protocol) || !url.hostname.includes(".")) {
            return "El dominio debe incluir http(s) y un host valido.";
        }
    } catch {
        return "El dominio debe ser una URL valida, por ejemplo https://sitio.com.";
    }

    if (!borrador.target.trim()) {
        return "Selecciona un VPS destino.";
    }

    return null;
}

export function useModalAgregarSitio({ abierto, targets, targetActivo, onCerrar, onCreado }: UseModalAgregarSitioProps) {
    const [borrador, setBorrador] = useState<CrearSitioRequest>(() => borradorInicial(targetActivo));
    const [enviando, setEnviando] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const opcionesTargets = useMemo(() => targets.map((target) => ({
        valor: target.name,
        etiqueta: target.name,
        detalle: `${target.host} · ${target.site_count} sitios`,
    })), [targets]);

    useEffect(() => {
        if (abierto) {
            setBorrador(borradorInicial(targetActivo));
            setError(null);
        }
    }, [abierto, targetActivo]);

    function actualizarBorrador(campo: keyof CrearSitioRequest, valor: string) {
        setBorrador((actual) => ({ ...actual, [campo]: valor }));
    }

    async function crearSitio() {
        const errorValidacion = validarBorrador(borrador);
        if (errorValidacion) {
            setError(errorValidacion);
            return;
        }

        setEnviando(true);
        setError(null);
        try {
            const resultado = await ejecutarComandoGui<ResultadoOperacion>("create_site", {
                ...borrador,
                name: borrador.name.trim(),
                domain: borrador.domain.trim(),
            });
            onCreado(resultado.datos);
            onCerrar();
        } catch (err) {
            setError(err instanceof Error ? err.message : String(err));
        } finally {
            setEnviando(false);
        }
    }

    return { borrador, enviando, error, opcionesTargets, actualizarBorrador, crearSitio };
}