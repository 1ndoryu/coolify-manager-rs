import { Settings2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { claseModoCliente, etiquetaModoCliente, type ModoCliente } from "../servicios/clienteCoolify";
import type { TargetResumen } from "../tipos";
import { SelectorPersonalizado } from "./ui/SelectorPersonalizado";

interface VistaAjustesProps {
    targets: TargetResumen[];
    targetActivo: string;
    configPath: string;
    modoCliente: ModoCliente;
}

interface BorradorSitio {
    nombre: string;
    dominio: string;
    template: string;
    target: string;
}

const OPCIONES_TEMPLATE = [
    { valor: "wordpress", etiqueta: "WordPress" },
    { valor: "rust", etiqueta: "Rust" },
    { valor: "kamples", etiqueta: "Kamples" },
];

export function VistaAjustes({ targets, targetActivo, configPath, modoCliente }: VistaAjustesProps) {
    const [borrador, setBorrador] = useState<BorradorSitio>({
        nombre: "nuevo-sitio",
        dominio: "https://example.com",
        template: "wordpress",
        target: targetActivo,
    });

    function actualizarBorrador(campo: keyof BorradorSitio, valor: string) {
        setBorrador((actual) => ({ ...actual, [campo]: valor }));
    }

    const opcionesTargets = useMemo(() => targets.map((item) => ({
        valor: item.name,
        etiqueta: item.name,
        detalle: `${item.host} · ${item.site_count} sitios`,
    })), [targets]);

    const comandoNuevoSitio = useMemo(() => (
        `coolify-manager.exe new --name ${borrador.nombre || "nuevo-sitio"} --domain ${borrador.dominio || "https://example.com"} --template ${borrador.template} --target ${borrador.target}`
    ), [borrador]);

    useEffect(() => {
        actualizarBorrador("target", targetActivo);
    }, [targetActivo]);

    return (
        <div className="vistaConsola">
            <header className="barraSuperior">
                <div>
                    <h1 className="tituloPagina">Ajustes</h1>
                </div>
                <span className={`badge ${claseModoCliente(modoCliente)}`}>{etiquetaModoCliente(modoCliente)}</span>
            </header>

            <section className="gridDosColumnas panelAjustes">
                <div className="tarjeta">
                    <h2 className="tarjetaTitulo"><Settings2 size={14} /> Configuración activa</h2>
                    <div className="listaLecturas">
                        <div className="filaLectura"><span>settings.json</span><pre>{configPath}</pre></div>
                        <div className="filaLectura"><span>Comando de desarrollo</span><pre>npm run dev</pre></div>
                        <div className="filaLectura"><span>Vista web sin Tauri</span><pre>npm run dev:web</pre></div>
                    </div>
                </div>
                <div className="tarjeta">
                    <h2 className="tarjetaTitulo">VPS / destinos</h2>
                    <div className="listaTargets">
                        {targets.map((item) => (
                            <div className="filaTarget" key={item.name}>
                                <span>{item.name}</span>
                                <strong>{item.host}</strong>
                                <small>{item.site_count} sitios · {item.coolify_url}</small>
                            </div>
                        ))}
                    </div>
                </div>
                <div className="tarjeta tarjetaAncha">
                    <h2 className="tarjetaTitulo">Agregar sitio</h2>
                    <div className="formularioAjustes">
                        <label>Nombre<input className="campoTexto" value={borrador.nombre} onChange={(event) => actualizarBorrador("nombre", event.target.value)} /></label>
                        <label>Dominio<input className="campoTexto" value={borrador.dominio} onChange={(event) => actualizarBorrador("dominio", event.target.value)} /></label>
                        <div><span>Plantilla</span><SelectorPersonalizado etiqueta="Plantilla" valor={borrador.template} opciones={OPCIONES_TEMPLATE} onCambiar={(valor) => actualizarBorrador("template", valor)} /></div>
                        <div><span>VPS</span><SelectorPersonalizado etiqueta="VPS del nuevo sitio" valor={borrador.target} opciones={opcionesTargets} placeholder="Sin VPS" onCambiar={(valor) => actualizarBorrador("target", valor)} /></div>
                    </div>
                    <pre className="comandoAjustes">{comandoNuevoSitio}</pre>
                </div>
            </section>
        </div>
    );
}