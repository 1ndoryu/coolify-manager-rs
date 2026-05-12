/*
 * [125A-5] VistaPortal — landing pública de vps.nakomi.studio.
 * Se rehizo contra la referencia real de relace.ai: Parabole, tokens exactos,
 * hero a dos columnas, imagen panorámica full-width con consola superpuesta,
 * secciones completas, testimonials, FAQ y footer CTA.
 */

import { useState } from "react";
import { Plus, Minus } from "lucide-react";
import "../estilos/portal.css";

interface VistaPortalProps {
    onAbrirLogin: () => void;
}

const HERO_IMG = "https://images.unsplash.com/photo-1500530855697-b586d89ba3ee?auto=format&fit=crop&w=1800&q=85";
const MODEL_IMG = "https://images.unsplash.com/photo-1500534314209-a25ddb2bd429?auto=format&fit=crop&w=1800&q=85";
const INFRA_IMG = "https://images.unsplash.com/photo-1464822759023-fed622ff2c3b?auto=format&fit=crop&w=1800&q=85";
const FOOTER_IMG = "https://images.unsplash.com/photo-1501785888041-af3ef285b470?auto=format&fit=crop&w=1800&q=85";

const TRUSTED = ["Coolify", "Docker", "Ubuntu", "Postgres", "Traefik", "Nakomi"];

const FEATURE_TRIO = [
    {
        title: "Repos",
        copy: "Servidores preparados para clonar, compilar y desplegar desde el primer acceso SSH.",
    },
    {
        title: "Provisioning",
        copy: "Alta manual con bootstrap de Docker, firewall y hostname listo para producción.",
    },
    {
        title: "Fast Apply",
        copy: "Recursos NVMe y CPU dedicados para aplicar cambios, reiniciar servicios y mover cargas rápido.",
    },
];

const VISUAL_SECTIONS = [
    {
        eyebrow: "Models",
        title: "SLMs de infraestructura como herramientas para operadores",
        copy: "Pequeños flujos automatizados para auditar, restaurar, reiniciar y observar servicios sin abrir sesiones largas ni repetir comandos manuales.",
        image: MODEL_IMG,
        imageAlt: "Distant rocky peaks beneath layered clouds in a warm-toned landscape.",
        overlay: "chart",
    },
    {
        eyebrow: "Infra",
        title: "Source control diseñado para los servidores que lo usan",
        copy: "Push/pull liviano desde sandboxes, health checks rápidos, backups y límites pensados para throughput alto en múltiples sitios.",
        image: INFRA_IMG,
        imageAlt: "Mountain ridge at sunset with trees in the foreground.",
        overlay: "status",
    },
];

const BLOCKS = [
    "modelos especializados",
    "retrieval rápido",
    "smart merge",
    "orquestación segura",
    "observabilidad",
    "deploy reproducible",
];

const TESTIMONIALS = [
    {
        quote: "Nakomi VPS nos dio una base simple para levantar servicios sin pelear con networking, firewall y bootstrap cada vez.",
        name: "Equipo Studio",
        role: "Operaciones en Nakomi",
    },
    {
        quote: "La parte más valiosa es que la máquina llega lista para trabajar: root, Docker, salud remota y una ruta clara para escalar.",
        name: "Cliente beta",
        role: "Founder técnico",
    },
];

const FAQ = [
    {
        q: "¿Por qué Nakomi VPS?",
        a: "Porque combina VPS dedicados con una capa operativa pensada para proyectos reales: bootstrap limpio, acceso root, Docker, health checks y soporte manual antes del alta.",
    },
    {
        q: "¿Cuándo se activa el VPS?",
        a: "El alta no es automática. Revisamos cada compra manualmente y el tiempo habitual es menos de 24 h en días laborables.",
    },
    {
        q: "¿Qué incluye el bootstrap inicial?",
        a: "Docker activo, firewall ufw con puertos 22, 80 y 443, hostname configurado y MOTD con tus recursos de hardware.",
    },
    {
        q: "¿Puedo cancelar en cualquier momento?",
        a: "Sí. La suscripción se cancela desde el panel y el servidor se desprovisiona al finalizar el ciclo de facturación actual.",
    },
    {
        q: "¿Qué sistema operativo incluye?",
        a: "Ubuntu 22.04 LTS por defecto. Si necesitas otra distribución, contáctanos antes de completar la compra.",
    },
    {
        q: "¿Qué tan rápido es el onboarding?",
        a: "Puedes empezar en minutos tras recibir el acceso. Docker, firewall y hostname ya están configurados.",
    },
];

function scrollTo(id: string): void {
    document.getElementById(id)?.scrollIntoView({ behavior: "smooth" });
}

function ConsoleOverlay() {
    return (
        <div className="vpsConsole">
            <div className="vpsConsoleBar">
                <span />
                <span />
                <span />
                <p>vps.nakomi.studio</p>
            </div>
            <pre className="vpsConsoleCode">
                <code>{`... async function deploy({ service }) {
  const health = await coolify.health(service)

  if (!health.ok) {
    await backups.restoreLatest(service)
    return { status: "restored" }
  }

  await docker.compose.pull(service)
  await docker.compose.up(service)

  return { status: "online" }
} ...`}</code>
            </pre>
        </div>
    );
}

function VisualOverlay({ type }: { type: string }) {
    if (type === "chart") {
        return (
            <div className="vpsOverlayChart" aria-hidden="true">
                <div className="vpsChartLine vpsChartLineUno" />
                <div className="vpsChartLine vpsChartLineDos" />
                <div className="vpsChartLine vpsChartLineTres" />
                <div className="vpsChartAxis">Latency</div>
                <div className="vpsChartAxis vpsChartAxisBottom">Deploy steps</div>
            </div>
        );
    }

    return (
        <div className="vpsOverlayStatus" aria-hidden="true">
            {['Status', 'Execution', 'Startup', 'Enqueued'].map((label, index) => (
                <div key={label} className="vpsStatusRow">
                    <span>{label}</span>
                    <div>
                        <i className={`vpsStatusBar vpsStatusBar${index + 1}`} />
                    </div>
                </div>
            ))}
        </div>
    );
}

export function VistaPortal({ onAbrirLogin }: VistaPortalProps) {
    const [faqAbierto, setFaqAbierto] = useState<number>(0);

    return (
        <div className="vpsPortal">
            <nav className="vpsNav">
                <div className="vpsNavInner">
                    <a className="vpsLogo" href="#top" aria-label="Nakomi VPS home">
                        <span>Nakomi</span>
                        <strong>VPS</strong>
                    </a>
                    <div className="vpsNavLinks">
                        <button onClick={() => scrollTo("docs")}>docs</button>
                        <button onClick={() => scrollTo("blog")}>Blog</button>
                        <button onClick={() => scrollTo("pricing")}>Pricing</button>
                        <button onClick={() => scrollTo("about")}>about us</button>
                    </div>
                    <div className="vpsNavActions">
                        <button className="vpsNavPlain" onClick={onAbrirLogin}>app</button>
                        <button className="vpsBtnMini" onClick={onAbrirLogin}>Get a Demo</button>
                    </div>
                </div>
            </nav>

            <main id="top">
                <section className="vpsHero">
                    <div className="vpsHeroCopy">
                        <h1>VPS built for shipping agents</h1>
                        <div className="vpsHeroSide">
                            <p>
                                Source control for servers with fast provisioning, dedicated resources,
                                and operational tools you can run on any repo.
                            </p>
                            <div className="vpsHeroButtons">
                                <button className="vpsBtnPrimary" onClick={onAbrirLogin}>Get a Demo</button>
                                <button className="vpsBtnSecondary" onClick={() => scrollTo("pricing")}>Sign Up for Free</button>
                            </div>
                        </div>
                    </div>

                    <div className="vpsTrusted">
                        <p>Trusted by the best leading stacks:</p>
                        <div className="vpsTrustedTrack">
                            {[...TRUSTED, ...TRUSTED].map((brand, index) => (
                                <span key={`${brand}-${index}`}>{brand}</span>
                            ))}
                        </div>
                    </div>

                    <div className="vpsHeroMedia">
                        <img src={HERO_IMG} alt="Wide mountain range beneath a cloudy sky with infrastructure overlay." />
                        <ConsoleOverlay />
                    </div>
                </section>

                <section id="docs" className="vpsFeatureIntro">
                    <div className="vpsFeatureIntroHead">
                        <p className="vpsEyebrow">Features</p>
                        <h2>Everything you need for autonomous VPS ops</h2>
                    </div>
                    <div className="vpsFeatureIntroBody">
                        <div className="vpsFeatureCards">
                            {FEATURE_TRIO.map((feature) => (
                                <article key={feature.title} className="vpsFeatureCard">
                                    <h3>{feature.title}</h3>
                                    <p>{feature.copy}</p>
                                </article>
                            ))}
                        </div>
                        <div className="vpsOrbitPanel" aria-hidden="true">
                            <div className="vpsOrbitDisc" />
                            <div className="vpsOrbitCard vpsOrbitCardUno">deploy</div>
                            <div className="vpsOrbitCard vpsOrbitCardDos">health</div>
                            <div className="vpsOrbitCard vpsOrbitCardTres">backup</div>
                        </div>
                    </div>
                </section>

                {VISUAL_SECTIONS.map((section) => (
                    <section key={section.eyebrow} id={section.eyebrow === "Models" ? "blog" : "about"} className="vpsVisualSection">
                        <div className="vpsVisualText">
                            <p className="vpsEyebrow">{section.eyebrow}</p>
                            <h2>{section.title}</h2>
                            <p>{section.copy}</p>
                        </div>
                        <div className="vpsVisualMedia">
                            <img src={section.image} alt={section.imageAlt} />
                            <VisualOverlay type={section.overlay} />
                        </div>
                    </section>
                ))}

                <section className="vpsBlocks">
                    <div className="vpsBlocksHead">
                        <p className="vpsEyebrow">Features</p>
                        <h2>Building blocks for reliability and scale</h2>
                        <p>VPS provisioning with codebase retrieval, utility automations and task-specific operations you can run on any service.</p>
                    </div>
                    <div className="vpsBlocksGrid">
                        {BLOCKS.map((block, index) => (
                            <article key={block} className="vpsBlockCard">
                                <h3>{block}</h3>
                                <p>No. {index + 1}</p>
                            </article>
                        ))}
                    </div>
                </section>

                <section id="pricing" className="vpsPricing">
                    <div className="vpsPricingHead">
                        <p className="vpsEyebrow">Pricing</p>
                        <h2>Plans for every workload</h2>
                        <p>Monthly billing, manual approval and clean cancellation from your panel.</p>
                    </div>
                    <div className="vpsPricingGrid">
                        {[
                            ["VPS 1", "$6.88", "1 vCPU", "2 GB RAM", "40 GB NVMe"],
                            ["VPS 2", "$11.99", "2 vCPU", "4 GB RAM", "60 GB NVMe"],
                            ["VPS 3", "$20.49", "4 vCPU", "8 GB RAM", "100 GB NVMe"],
                            ["VPS 4", "$36.99", "6 vCPU", "16 GB RAM", "200 GB NVMe"],
                        ].map((plan) => (
                            <article key={plan[0]} className="vpsPriceCard">
                                <p>{plan[0]}</p>
                                <h3>{plan[1]}</h3>
                                <ul>
                                    {plan.slice(2).map((item) => <li key={item}>{item}</li>)}
                                    <li>Root + SSH</li>
                                    <li>Docker preinstalado</li>
                                </ul>
                                <button className="vpsBtnPrimary" onClick={onAbrirLogin}>Get a Demo</button>
                            </article>
                        ))}
                    </div>
                </section>

                <section className="vpsTestimonials">
                    <div className="vpsTestimonialsHead">
                        <p className="vpsEyebrow">Testimonials</p>
                        <h2>Trusted by trailblazers</h2>
                    </div>
                    <div className="vpsTestimonialsGrid">
                        {TESTIMONIALS.map((testimonial) => (
                            <article key={testimonial.name} className="vpsTestimonialCard">
                                <p>{testimonial.quote}</p>
                                <div>
                                    <strong>{testimonial.name}</strong>
                                    <span>{testimonial.role}</span>
                                </div>
                            </article>
                        ))}
                    </div>
                </section>

                <section className="vpsFaq">
                    <div className="vpsFaqHead">
                        <p className="vpsEyebrow">FAQs</p>
                        <h2>Frequently asked questions</h2>
                    </div>
                    <ul className="vpsFaqList">
                        {FAQ.map((item, index) => (
                            <li key={item.q}>
                                <button onClick={() => setFaqAbierto(faqAbierto === index ? -1 : index)}>
                                    <span>{item.q}</span>
                                    {faqAbierto === index ? <Minus size={14} /> : <Plus size={14} />}
                                </button>
                                {faqAbierto === index && <p>{item.a}</p>}
                            </li>
                        ))}
                    </ul>
                </section>
            </main>

            <footer className="vpsFooter">
                <div className="vpsFooterCta">
                    <div>
                        <h2>Get started in <strong>minutes</strong></h2>
                        <p>Try the panel, choose your VPS tier and start shipping infrastructure from a clean baseline.</p>
                    </div>
                    <div className="vpsFooterButtons">
                        <button className="vpsBtnPrimary" onClick={onAbrirLogin}>Get a Demo</button>
                        <button className="vpsBtnSecondary" onClick={onAbrirLogin}>Sign Up for Free</button>
                    </div>
                </div>
                <div className="vpsFooterImage">
                    <img src={FOOTER_IMG} alt="Panoramic view of jagged mountain peaks fading into a muted horizon." />
                </div>
                <div className="vpsFooterLinks">
                    <div>
                        <a href="https://nakomi.studio">nakomi studio</a>
                        <button onClick={() => scrollTo("about")}>about us</button>
                        <a href="mailto:info@nakomi.studio">email us</a>
                    </div>
                    <div>
                        <p>Copyright ©</p>
                        <p>{new Date().getFullYear()}</p>
                        <p>Nakomi</p>
                    </div>
                    <div>
                        <a href="/legal/terms-of-use">Terms of Use</a>
                        <a href="/politica-privacidad">Privacy Policy</a>
                    </div>
                </div>
            </footer>
        </div>
    );
}
