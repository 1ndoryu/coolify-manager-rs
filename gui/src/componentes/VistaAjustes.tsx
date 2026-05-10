import { Settings2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { claseModoCliente, ejecutarComandoGui, etiquetaModoCliente, type ModoCliente } from "../servicios/clienteCoolify";
import type { RespuestaTargets, TargetResumen } from "../tipos";

export function VistaAjustes() {
    const [targets, setTargets] = useState<TargetResumen[]>([]);
    const [configPath, setConfigPath] = useState("--");
    const [modoCliente, setModoCliente] = useState<ModoCliente>("local");
    const [nombre, setNombre] = useState("nuevo-sitio");
    const [dominio, setDominio] = useState("https://example.com");
    const [template, setTemplate] = useState("wordpress");
    const [target, setTarget] = useState("default");

    const comandoNuevoSitio = useMemo(() => (
        `coolify-manager.exe new --name ${nombre || "nuevo-sitio"} --domain ${dominio || "https://example.com"} --template ${template} --target ${target}`
    ), [dominio, nombre, target, template]);

    useEffect(() => {
        async function cargar() {
            const targetsResultado = await ejecutarComandoGui<RespuestaTargets>("list_targets");
            setModoCliente(targetsResultado.modo);
            setTargets(targetsResultado.datos.targets);
            setConfigPath(targetsResultado.datos.config_path);
            setTarget(targetsResultado.datos.default_target);
        }

        void cargar();
    }, []);

    return (
        <div className="vistaConsola">
            <header className="barraSuperior">
                <div>
                    <div className="rutaPagina">Coolify / Ajustes</div>
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
                        <label>Nombre<input className="campoTexto" value={nombre} onChange={(event) => setNombre(event.target.value)} /></label>
                        <label>Dominio<input className="campoTexto" value={dominio} onChange={(event) => setDominio(event.target.value)} /></label>
                        <label>Plantilla<select className="selectorCompacto" value={template} onChange={(event) => setTemplate(event.target.value)}><option value="wordpress">WordPress</option><option value="rust">Rust</option><option value="kamples">Kamples</option></select></label>
                        <label>VPS<select className="selectorCompacto" value={target} onChange={(event) => setTarget(event.target.value)}>{targets.map((item) => <option key={item.name} value={item.name}>{item.name}</option>)}</select></label>
                    </div>
                    <pre className="comandoAjustes">{comandoNuevoSitio}</pre>
                </div>
            </section>
        </div>
    );
}