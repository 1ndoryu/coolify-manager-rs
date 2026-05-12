/*
 * [125A-5] VistaPortal — landing page pública de vps.nakomi.studio.
 * Estilo inspirado en relace.ai: serif headings, fondo crema, botones pill naranja,
 * nav uppercase minimal, secciones left-aligned, bandas de paisaje CSS.
 */

import { useState } from "react";
import { Check, Plus, Minus } from "lucide-react";
import "../estilos/portal.css";

interface VistaPortalProps {
    onAbrirLogin: () => void;
}

const FEATURES_LISTA = [
    { titulo: "Recursos dedicados", desc: "CPU, RAM y NVMe asignados exclusivamente a tu instancia. Sin vecinos ruidosos." },
    { titulo: "Alta manual garantizada", desc: "Revisamos cada compra antes de provisionar. Menos fraude, mayor calidad de red." },
    { titulo: "NVMe SSD en todos los planes", desc: "Storage rápido para bases de datos, colas y despliegues de producción." },
    { titulo: "Root + SSH desde el día uno", desc: "Acceso root completo. Instala, configura y despliega lo que necesites." },
    { titulo: "Bootstrap listo en minutos", desc: "Docker, firewall ufw y hostname ya configurados al provisionar." },
    { titulo: "Escalado claro y sin sorpresas", desc: "Cambia de plan cuando lo necesites. Precios transparentes." },
];

const BLOQUES = [
    "Recursos exclusivos",
    "Aprovisionamiento rápido",
    "Alta manual",
    "Baja latencia",
    "Integración sencilla",
    "Disponibilidad garantizada",
];

const PLANES = [
    { nombre: "VPS 1", precio: "$6.88", desc: "Para proyectos ligeros y bots.", destacado: false, features: ["1 vCPU dedicado", "2 GB RAM", "40 GB NVMe SSD", "Root + SSH", "Docker preinstalado"] },
    { nombre: "VPS 2", precio: "$11.99", desc: "Para apps web y APIs.", destacado: true, features: ["2 vCPU dedicados", "4 GB RAM", "60 GB NVMe SSD", "Root + SSH", "Docker preinstalado", "Soporte prioritario"] },
    { nombre: "VPS 3", precio: "$20.49", desc: "Para cargas de trabajo mayores.", destacado: false, features: ["4 vCPU dedicados", "8 GB RAM", "100 GB NVMe SSD", "Root + SSH", "Docker preinstalado"] },
    { nombre: "VPS 4", precio: "$36.99", desc: "Infraestructura seria.", destacado: false, features: ["6 vCPU dedicados", "16 GB RAM", "200 GB NVMe SSD", "Root + SSH", "Docker preinstalado", "IPv6 disponible"] },
];

const FAQ = [
    { q: "¿Cuándo se activa el VPS?", a: "El alta no es automática: revisamos cada compra manualmente. El tiempo habitual es menos de 24 h en días laborables." },
    { q: "¿Qué incluye el bootstrap inicial?", a: "Docker instalado y activo, firewall ufw con puertos 22, 80 y 443 abiertos, hostname configurado y MOTD con tus recursos de hardware." },
    { q: "¿Puedo cancelar en cualquier momento?", a: "Sí. La suscripción se cancela desde tu panel y el servidor se desprovisiona al finalizar el ciclo de facturación actual." },
    { q: "¿Qué sistema operativo incluye?", a: "Ubuntu 22.04 LTS por defecto. Si necesitas otra distribución, contáctanos antes de completar la compra." },
    { q: "¿Tienen IPv6?", a: "IPv4 dedicada en todos los planes. IPv6 disponible bajo consulta para VPS 3 y VPS 4." },
    { q: "¿Qué tan rápido es el onboarding?", a: "Recibes acceso SSH en menos de 24 h. Docker, firewall y hostname ya configurados — despliega desde el primer día." },
];

function scrollTo(id: string): void {
    document.getElementById(id)?.scrollIntoView({ behavior: "smooth" });
}

export function VistaPortal({ onAbrirLogin }: VistaPortalProps) {
    const [faqAbierto, setFaqAbierto] = useState<number | null>(null);

    return (
        <div className="vpsPortal">

            {/* ── Nav ── */}
            <nav className="vpsNav">
                <div className="vpsNavWrap">
                    <a href="#" className="vpsNavLogo">Nakomi<span>VPS</span></a>
                    <div className="vpsNavLinks">
                        <button className="vpsNavLink" onClick={() => scrollTo("features")}>CARACTERÍSTICAS</button>
                        <button className="vpsNavLink" onClick={() => scrollTo("precios")}>PRECIOS</button>
                        <button className="vpsNavLink" onClick={() => scrollTo("faq")}>FAQ</button>
                    </div>
                    <div className="vpsNavDer">
                        <button className="vpsNavBtn" onClick={onAbrirLogin}>ACCEDER</button>
                    </div>
                </div>
            </nav>

            {/* ── Hero ── */}
            <section className="vpsHero">
                <div className="vpsHeroInner">
                    <div className="vpsHeroTexto">
                        <p className="vpsLabel">Infraestructura propia · Sin intermediarios</p>
                        <h1 className="vpsHeroTitulo">
                            Servidores VPS<br />
                            <em>dedicados y listos</em>
                        </h1>
                        <p className="vpsHeroSub">
                            Root SSH, Docker preinstalado y alta manual en menos de 24 h.
                            Planes desde <strong>$6.88/mes</strong>.
                        </p>
                        <div className="vpsHeroBtns">
                            <button className="vpsBtnPrimario" onClick={onAbrirLogin}>CONTRATAR VPS</button>
                            <button className="vpsBtnOutline" onClick={() => scrollTo("precios")}>VER PRECIOS</button>
                        </div>
                    </div>

                    <div className="vpsHeroVisual">
                        <div className="vpsTerminal">
                            <div className="vpsTerminalBar">
                                <span className="vpsTerminalDot" />
                                <span className="vpsTerminalDot" />
                                <span className="vpsTerminalDot" />
                                <span className="vpsTerminalTitulo">ssh root@vps.nakomi.studio</span>
                            </div>
                            <div className="vpsTerminalBody">
                                <div className="vpsTerminalRow"><span className="vtP">$</span><span className="vtC">neofetch --off 2&gt;/dev/null | head -8</span></div>
                                <div className="vpsTerminalRow vtS">OS: Ubuntu 22.04.3 LTS x86_64</div>
                                <div className="vpsTerminalRow vtS">CPU: AMD EPYC 7282 (2) @ 2.8GHz</div>
                                <div className="vpsTerminalRow vtS">Memory: 1245MiB / 3894MiB</div>
                                <div className="vpsTerminalRow vtS">Disk (/): 12G / 59G</div>
                                <div className="vpsTerminalRow"><span className="vtP">$</span><span className="vtC">{"docker ps --format 'table {{.Names}}\\t{{.Status}}'"}</span></div>
                                <div className="vpsTerminalRow vtS">nginx       Up 5 days</div>
                                <div className="vpsTerminalRow vtS">postgres    Up 5 days</div>
                                <div className="vpsTerminalRow vtS">app         Up 14 hours</div>
                                <div className="vpsTerminalRow"><span className="vtP">$</span><span className="vtCursor">▌</span></div>
                            </div>
                        </div>
                    </div>
                </div>
            </section>

            {/* ── Banda paisaje 1 ── */}
            <div className="vpsPaisaje" />

            {/* ── Features ── */}
            <section id="features" className="vpsSeccion">
                <div className="vpsSeccionWrap">
                    <div className="vpsSeccionIzq">
                        <p className="vpsLabel">CARACTERÍSTICAS</p>
                        <h2 className="vpsSeccionTitulo">
                            Todo lo que necesitas para infraestructura real
                        </h2>
                        <p className="vpsSeccionTexto">
                            Servidores con recursos exclusivos, acceso root completo y bootstrap automatizado.
                            Sin contratos ni sorpresas en la factura.
                        </p>
                    </div>
                    <div className="vpsSeccionDer">
                        {FEATURES_LISTA.map((f) => (
                            <div key={f.titulo} className="vpsFeatureItem">
                                <h3 className="vpsFeatureItemTitulo">{f.titulo}</h3>
                                <p className="vpsFeatureItemDesc">{f.desc}</p>
                            </div>
                        ))}
                    </div>
                </div>
            </section>

            {/* ── Banda paisaje 2 ── */}
            <div className="vpsPaisaje vpsPaisaje2" />

            {/* ── Bloques ── */}
            <section className="vpsBloques">
                <div className="vpsBloquesInner">
                    <p className="vpsLabel" style={{ textAlign: "center" }}>BLOQUES</p>
                    <h2 className="vpsSeccionTituloC">Bloques de confiabilidad y escala</h2>
                    <p className="vpsSeccionSubC">
                        Infraestructura con recursos exclusivos, SLMs de alta velocidad<br />
                        y acceso completo que puedes usar en cualquier proyecto.
                    </p>
                    <div className="vpsBloquesGrid">
                        {BLOQUES.map((b, i) => (
                            <div key={b} className="vpsBloque">
                                <span className="vpsBloquNum">NO. {i + 1}</span>
                                <p className="vpsBloquTitulo">{b}</p>
                            </div>
                        ))}
                    </div>
                </div>
            </section>

            {/* ── Precios ── */}
            <section id="precios" className="vpsPrecios">
                <div className="vpsPreciosInner">
                    <p className="vpsLabel">PRECIOS</p>
                    <h2 className="vpsSeccionTitulo vpsSeccionTituloPrecios">Planes mensuales</h2>
                    <p className="vpsSeccionTexto">Facturación mensual. Sin contratos. Cancela cuando quieras.</p>
                    <div className="vpsPreciosGrid">
                        {PLANES.map((plan) => (
                            <div key={plan.nombre} className={`vpsPrecioCard${plan.destacado ? " vpsPrecioCardTop" : ""}`}>
                                {plan.destacado && <div className="vpsPrecioTag">Popular</div>}
                                <p className="vpsPrecioNombre">{plan.nombre}</p>
                                <div className="vpsPrecioValor">
                                    <span className="vpsPrecioNum">{plan.precio}</span>
                                    <span className="vpsPrecioMes">/mes</span>
                                </div>
                                <p className="vpsPrecioDesc">{plan.desc}</p>
                                <ul className="vpsPrecioLista">
                                    {plan.features.map((f) => (
                                        <li key={f}>
                                            <Check size={11} />{f}
                                        </li>
                                    ))}
                                </ul>
                                <button
                                    className={plan.destacado ? "vpsBtnPrimario" : "vpsBtnOutline"}
                                    onClick={onAbrirLogin}
                                >
                                    CONTRATAR
                                </button>
                            </div>
                        ))}
                    </div>
                </div>
            </section>

            {/* ── FAQ ── */}
            <section id="faq" className="vpsFaq">
                <div className="vpsFaqWrap">
                    <div className="vpsFaqIzq">
                        <p className="vpsLabel">FAQ</p>
                        <h2 className="vpsFaqTitulo">
                            Preguntas<br />frecuentes
                        </h2>
                    </div>
                    <div className="vpsFaqDer">
                        {FAQ.map((item, i) => (
                            <div key={i} className={`vpsFaqItem${faqAbierto === i ? " vpsFaqOpen" : ""}`}>
                                <button
                                    className="vpsFaqQ"
                                    onClick={() => setFaqAbierto(faqAbierto === i ? null : i)}
                                >
                                    <span>{item.q}</span>
                                    {faqAbierto === i
                                        ? <Minus size={14} />
                                        : <Plus size={14} />}
                                </button>
                                {faqAbierto === i && <p className="vpsFaqA">{item.a}</p>}
                            </div>
                        ))}
                    </div>
                </div>
            </section>

            {/* ── CTA final ── */}
            <section className="vpsCtaFinal">
                <div className="vpsCtaInner">
                    <h2 className="vpsCtaTitulo">
                        Empieza en <em>minutos</em>
                    </h2>
                    <p className="vpsCtaSub">
                        Solicita tu VPS — revisamos tu petición y te respondemos en menos de 24 h.
                    </p>
                    <div className="vpsCtaBtns">
                        <button className="vpsBtnPrimario" onClick={onAbrirLogin}>CONTRATAR VPS</button>
                        <button className="vpsBtnOutlineClaro" onClick={() => scrollTo("precios")}>VER PLANES</button>
                    </div>
                </div>
                <div className="vpsCtaPaisaje" />
            </section>

            {/* ── Footer ── */}
            <footer className="vpsFooter">
                <div className="vpsFooterWrap">
                    <div className="vpsFooterLinks">
                        <a href="https://nakomi.studio">NAKOMI STUDIO</a>
                        <a href="#" onClick={(e) => { e.preventDefault(); onAbrirLogin(); }}>PANEL</a>
                        <a href="/politica-privacidad">PRIVACIDAD</a>
                    </div>
                    <span className="vpsFooterCopy">© {new Date().getFullYear()} Nakomi Studio</span>
                </div>
            </footer>
        </div>
    );
}
