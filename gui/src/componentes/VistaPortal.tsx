/*
 * [125A-5] VistaPortal — landing page pública de vps.nakomi.studio.
 * Se muestra cuando el usuario no está autenticado.
 * Login se abre como modal al hacer clic en "Acceder".
 * Sin LayoutPagina ni Header del panel — standalone con nav y footer propios.
 */

import { useState } from "react";
import { Cpu, Shield, HardDrive, TerminalSquare, Activity, Server, ChevronDown, Check } from "lucide-react";
import "../estilos/portal.css";

interface VistaPortalProps {
    onAbrirLogin: () => void;
}

const FEATURES = [
    { icono: Cpu, titulo: "Recursos dedicados", desc: "CPU, RAM y NVMe asignados exclusivamente a tu instancia. Sin vecinos ruidosos." },
    { icono: Shield, titulo: "Alta manual", desc: "Revisamos cada compra antes de provisionar. Menos fraude, mayor calidad de red." },
    { icono: HardDrive, titulo: "NVMe SSD", desc: "Storage rápido para bases de datos, colas y despliegues de producción." },
    { icono: TerminalSquare, titulo: "Root + SSH", desc: "Acceso root completo desde el primer día. Instala lo que necesites." },
    { icono: Activity, titulo: "Bootstrap listo", desc: "Docker, firewall y hostname ya configurados. Despliega en minutos." },
    { icono: Server, titulo: "Escalado limpio", desc: "Cambia de plan cuando lo necesites. Precios transparentes sin sorpresas." },
];

const PLANES = [
    { nombre: "VPS 1", precio: "$6.88", periodo: "/mes", desc: "Para proyectos ligeros y bots.", destacado: false, features: ["1 vCPU dedicado", "2 GB RAM", "40 GB NVMe SSD", "Root + SSH", "Docker preinstalado"] },
    { nombre: "VPS 2", precio: "$11.99", periodo: "/mes", desc: "Para apps web y APIs.", destacado: true, features: ["2 vCPU dedicados", "4 GB RAM", "60 GB NVMe SSD", "Root + SSH", "Docker preinstalado", "Soporte prioritario"] },
    { nombre: "VPS 3", precio: "$20.49", periodo: "/mes", desc: "Para cargas de trabajo mayores.", destacado: false, features: ["4 vCPU dedicados", "8 GB RAM", "100 GB NVMe SSD", "Root + SSH", "Docker preinstalado"] },
    { nombre: "VPS 4", precio: "$36.99", periodo: "/mes", desc: "Infraestructura seria.", destacado: false, features: ["6 vCPU dedicados", "16 GB RAM", "200 GB NVMe SSD", "Root + SSH", "Docker preinstalado", "IPv6 disponible"] },
];

const FAQ = [
    { q: "¿Cuándo se activa el VPS?", a: "El alta no es automática: revisamos cada compra manualmente. El tiempo habitual es menos de 24 h en días laborables." },
    { q: "¿Qué incluye el bootstrap inicial?", a: "Docker instalado y activo, firewall ufw con puertos 22, 80 y 443 abiertos, hostname configurado y MOTD con tus recursos de hardware." },
    { q: "¿Puedo cancelar en cualquier momento?", a: "Sí. La suscripción se cancela desde tu panel y el servidor se desprovisiona al finalizar el ciclo de facturación actual." },
    { q: "¿Qué sistema operativo incluye?", a: "Ubuntu 22.04 LTS por defecto. Si necesitas otra distribución contáctanos antes de completar la compra." },
    { q: "¿Tienen IPv6?", a: "IPv4 dedicada en todos los planes. IPv6 disponible bajo consulta para VPS 3 y VPS 4." },
];

function scrollTo(id: string): void {
    document.getElementById(id)?.scrollIntoView({ behavior: "smooth" });
}

export function VistaPortal({ onAbrirLogin }: VistaPortalProps) {
    const [faqAbierto, setFaqAbierto] = useState<number | null>(null);

    return (
        <div className="vpsPortal">
            {/* Nav */}
            <nav className="vpsNav">
                <div className="vpsNavContenido">
                    <a href="#" className="vpsNavLogo">
                        <span className="vpsNavLogoMarca">Nakomi</span>
                        <span className="vpsNavLogoProducto">VPS</span>
                    </a>
                    <div className="vpsNavLinks">
                        <button className="vpsNavLink" onClick={() => scrollTo("features")}>Características</button>
                        <button className="vpsNavLink" onClick={() => scrollTo("precios")}>Precios</button>
                        <button className="vpsNavLink" onClick={() => scrollTo("faq")}>FAQ</button>
                    </div>
                    <div className="vpsNavAcciones">
                        <button className="vpsNavAcceder" onClick={onAbrirLogin}>Acceder</button>
                    </div>
                </div>
            </nav>

            {/* Hero */}
            <section className="vpsHero">
                <div className="vpsHeroContenido">
                    <div className="vpsHeroBadge">Infraestructura propia · Sin intermediarios</div>
                    <h1 className="vpsHeroTitulo">
                        Servidores VPS<br />
                        <span className="vpsHeroAcento">dedicados y listos</span>
                    </h1>
                    <p className="vpsHeroSub">
                        Root SSH, Docker preinstalado y alta manual en menos de 24 h.<br />
                        Planes desde <strong>$6.88/mes</strong>.
                    </p>
                    <div className="vpsHeroCtas">
                        <button className="vpsBtnPrimario" onClick={() => scrollTo("precios")}>Ver planes →</button>
                        <button className="vpsBtnSecundario" onClick={() => scrollTo("features")}>Cómo funciona</button>
                    </div>
                </div>

                {/* Demo terminal */}
                <div className="vpsTerminal">
                    <div className="vpsTerminalHeader">
                        <span className="vpsTerminalDot" style={{ background: "#ff5f57" }} />
                        <span className="vpsTerminalDot" style={{ background: "#febc2e" }} />
                        <span className="vpsTerminalDot" style={{ background: "#28c840" }} />
                        <span className="vpsTerminalLabel">ssh root@vps.nakomi.studio</span>
                    </div>
                    <div className="vpsTerminalCuerpo">
                        <div className="vpsTerminalLinea"><span className="vpsTerminalPrompt">$</span> <span className="vpsTerminalCodigo">uname -m && free -h && df -h /</span></div>
                        <div className="vpsTerminalLinea vpsTerminalSalida">x86_64</div>
                        <div className="vpsTerminalLinea vpsTerminalSalida">Mem: 3.8Gi used / 3.9Gi free of 7.6Gi</div>
                        <div className="vpsTerminalLinea vpsTerminalSalida">/dev/sda1: 12G used / 46G avail / 60G total</div>
                        <div className="vpsTerminalLinea"><span className="vpsTerminalPrompt">$</span> <span className="vpsTerminalCodigo">docker ps --format "table {{"{{"}}{{".Names"}}{{"}}"}}\\t{{"{{"}}{{".Status"}}{{"}}"}}"</span></div>
                        <div className="vpsTerminalLinea vpsTerminalSalida">nginx       Up 3 days</div>
                        <div className="vpsTerminalLinea vpsTerminalSalida">postgres    Up 3 days</div>
                        <div className="vpsTerminalLinea vpsTerminalSalida">app         Up 2 hours</div>
                        <div className="vpsTerminalLinea"><span className="vpsTerminalPrompt">$</span> <span className="vpsTerminalCursor">▌</span></div>
                    </div>
                </div>
            </section>

            {/* Features */}
            <section id="features" className="vpsFeatures">
                <h2 className="vpsSectionTitulo">Todo lo que necesitas, nada más</h2>
                <div className="vpsFeaturesGrid">
                    {FEATURES.map(({ icono: Icono, titulo, desc }) => (
                        <div key={titulo} className="vpsFeatureCard">
                            <Icono size={18} className="vpsFeatureIcono" />
                            <h3 className="vpsFeatureTitulo">{titulo}</h3>
                            <p className="vpsFeatureDesc">{desc}</p>
                        </div>
                    ))}
                </div>
            </section>

            {/* Precios */}
            <section id="precios" className="vpsPrecios">
                <h2 className="vpsSectionTitulo">Planes</h2>
                <p className="vpsSectionSub">Facturación mensual. Sin contratos. Cancela cuando quieras.</p>
                <div className="vpsPreciosGrid">
                    {PLANES.map((plan) => (
                        <div key={plan.nombre} className={`vpsPrecioCard${plan.destacado ? " vpsPrecioCardDestacado" : ""}`}>
                            {plan.destacado && <div className="vpsPrecioBadge">Popular</div>}
                            <div className="vpsPrecioNombre">{plan.nombre}</div>
                            <div className="vpsPrecioValor">
                                <span className="vpsPrecioNumero">{plan.precio}</span>
                                <span className="vpsPrecioPeriodo">{plan.periodo}</span>
                            </div>
                            <p className="vpsPrecioDesc">{plan.desc}</p>
                            <ul className="vpsPrecioFeatures">
                                {plan.features.map((f) => (
                                    <li key={f} className="vpsPrecioFeature">
                                        <Check size={12} className="vpsPrecioCheck" />
                                        {f}
                                    </li>
                                ))}
                            </ul>
                            <button className={plan.destacado ? "vpsBtnPrimario" : "vpsBtnOutline"} onClick={onAbrirLogin}>
                                Contratar
                            </button>
                        </div>
                    ))}
                </div>
            </section>

            {/* FAQ */}
            <section id="faq" className="vpsFaq">
                <h2 className="vpsSectionTitulo">Preguntas frecuentes</h2>
                <div className="vpsFaqLista">
                    {FAQ.map((item, i) => (
                        <div key={i} className={`vpsFaqItem${faqAbierto === i ? " vpsFaqItemAbierto" : ""}`}>
                            <button className="vpsFaqPregunta" onClick={() => setFaqAbierto(faqAbierto === i ? null : i)}>
                                <span>{item.q}</span>
                                <ChevronDown size={16} className="vpsFaqChevron" />
                            </button>
                            {faqAbierto === i && <p className="vpsFaqRespuesta">{item.a}</p>}
                        </div>
                    ))}
                </div>
            </section>

            {/* CTA final */}
            <section className="vpsCtaFinal">
                <h2 className="vpsCtaFinalTitulo">Infraestructura lista en minutos.</h2>
                <p className="vpsCtaFinalSub">Solicita tu VPS hoy — revisamos tu petición y te respondemos en menos de 24 h.</p>
                <button className="vpsBtnPrimario" onClick={() => scrollTo("precios")}>Ver planes →</button>
            </section>

            {/* Footer */}
            <footer className="vpsFooter">
                <div className="vpsFooterContenido">
                    <span className="vpsFooterMarca">Nakomi VPS</span>
                    <div className="vpsFooterLinks">
                        <a href="https://nakomi.studio" className="vpsFooterLink">Nakomi Studio</a>
                        <a href="/politica-privacidad" className="vpsFooterLink">Privacidad</a>
                    </div>
                    <span className="vpsFooterCopy">© {new Date().getFullYear()} Nakomi Studio</span>
                </div>
            </footer>
        </div>
    );
}
