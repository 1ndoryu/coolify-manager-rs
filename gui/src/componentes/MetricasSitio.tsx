import { Activity } from "lucide-react";
import type { MetricaDespliegue } from "../tipos";

function formatearBytes(bytes: number): string {
    if (!Number.isFinite(bytes) || bytes <= 0) {
        return "--";
    }

    const unidades = ["B", "KB", "MB", "GB", "TB"];
    let valor = bytes;
    let indice = 0;
    while (valor >= 1024 && indice < unidades.length - 1) {
        valor /= 1024;
        indice += 1;
    }

    return `${valor >= 10 ? valor.toFixed(0) : valor.toFixed(1)} ${unidades[indice]}`;
}

export function MetricaCpu({ metrica }: { metrica?: MetricaDespliegue }) {
    if (!metrica || metrica.status !== "running") {
        return <span className="textoSuave">--</span>;
    }

    return (
        <span className="metricaCompacta" title={`${metrica.containers.length} contenedor(es)`}>
            <Activity size={13} /> {metrica.total_cpu_percent.toFixed(1)}%
        </span>
    );
}

export function MetricaRam({ metrica }: { metrica?: MetricaDespliegue }) {
    if (!metrica || metrica.status !== "running") {
        return <span className="textoSuave">--</span>;
    }

    return (
        <div className="metricaRam" title={`${formatearBytes(metrica.memory_used_bytes)} / ${formatearBytes(metrica.memory_limit_bytes)}`}>
            <meter className="barraMetrica" value={Math.min(metrica.memory_percent, 100)} max={100} />
            <span>{formatearBytes(metrica.memory_used_bytes)}</span>
        </div>
    );
}