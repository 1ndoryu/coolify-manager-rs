/*
 * [125A-5] VisualOverlay — overlays decorativos (chart / status) de las
 * secciones visuales de VistaPortal. Extraido de VistaPortal.tsx (Fase H):
 * sub-componente visual sin estado, solo aria-hidden.
 */

export function VisualOverlay({ type }: { type: string }) {
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
