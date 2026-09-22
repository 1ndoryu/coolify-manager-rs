/* Recomendaciones del auditor (119A-5 split control_plane_audit_manager). */

use super::tipos::ContainerStat;

#[allow(clippy::too_many_arguments)]
pub(super) fn build_recommendations(
    stats: &[ContainerStat],
    load_average: &str,
    coolify_process_summary: &str,
    supervisor_summary: &str,
    scheduler_summary: &str,
    horizon_summary: &str,
    failed_job_summary: &str,
    redis_summary: &str,
    queue_summary: &str,
    logs_summary: &str,
) -> Vec<String> {
    let mut recommendations = Vec::new();
    let first = stats.first();
    let load_1m = load_average
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(0.0);

    if let Some(first) = first {
        if first.name == "coolify" && first.cpu_percent >= 50.0 {
            recommendations.push(
                "El hotspot principal es el contenedor coolify, no el sitio WordPress de prueba. Conviene revisar jobs/scheduler del panel antes de tocar el workload alojado.".to_string(),
            );
        }
    }

    if coolify_process_summary.contains("php") {
        recommendations.push(
            "Dentro del contenedor coolify la carga cae en procesos PHP; revisar scheduler, colas y tareas internas del panel.".to_string(),
        );
    }

    if supervisor_summary.contains("RUNNING") {
        recommendations.push(
            "Supervisor tiene procesos activos en el control-plane; si la CPU sigue alta, correlacionar el proceso PHP caliente con el servicio supervisado correspondiente.".to_string(),
        );
    }

    if scheduler_summary.contains("ScheduledJobManager")
        && scheduler_summary.contains("ServerManagerJob")
    {
        recommendations.push(
            "El scheduler de Coolify está activo cada minuto para `ScheduledJobManager` y `ServerManagerJob`; si uno se vuelve caro, la presión es sostenida incluso sin tráfico del sitio alojado.".to_string(),
        );
    }

    if horizon_summary
        .to_ascii_lowercase()
        .contains("failed_jobs=")
        && !horizon_summary.contains("failed_jobs=0")
        && !horizon_summary.contains("failed_jobs=unknown")
    {
        recommendations.push(
            "Horizon o la cola tienen jobs fallidos pendientes; revisar esas fallas porque pueden reintentarse y sostener carga innecesaria.".to_string(),
        );
    }

    if failed_job_summary.contains("ConnectProxyToNetworksJob") {
        recommendations.push(
            "Los failed jobs recientes son `ConnectProxyToNetworksJob` con timeout. Eso apunta a costo/latencia al conectar redes del proxy desde el panel, no al WordPress alojado; conviene revisar attach de redes y llamadas SSH remotas de Coolify en este host.".to_string(),
        );
    }

    if queue_summary.contains("len=") && !queue_lengths_are_zero(queue_summary) {
        recommendations.push(
            "Redis muestra colas internas con backlog no trivial; el scheduler/Horizon puede estar gastando CPU simplemente en drenar o inspeccionar esa cola.".to_string(),
        );
    }

    if redis_summary.contains("blocked_clients=") && !redis_summary.contains("blocked_clients=0") {
        recommendations.push(
            "Redis tiene clientes bloqueados; eso refuerza que el cuello puede estar en colas/realtime mas que en el sitio alojado.".to_string(),
        );
    }

    if logs_summary.contains("ScheduledJobManager") {
        recommendations.push(
            "`ScheduledJobManager` aparece repetidamente en los logs del panel; en VPS2 conviene revisar si alguna tarea programada del control-plane se está volviendo lenta o se solapa.".to_string(),
        );
    }

    if logs_summary.contains("horizon:snapshot") {
        recommendations.push(
            "`horizon:snapshot` también aparece en el contenedor coolify. Si tarda varios segundos solo en VPS2, el panel puede estar más penalizado por I/O o por jobs internos que en VPS1.".to_string(),
        );
    }

    if logs_summary.contains("PushServerUpdateJob") {
        recommendations.push(
            "`PushServerUpdateJob` apareció en los logs del panel. Si solo se ve en VPS2 o tarda más allí, puede estar añadiendo trabajo extra del control-plane sobre ese host.".to_string(),
        );
    }

    if logs_summary.to_ascii_lowercase().contains("timeout")
        || logs_summary.to_ascii_lowercase().contains("failed")
        || logs_summary.to_ascii_lowercase().contains("exception")
    {
        recommendations.push(
            "Los logs recientes de coolify muestran errores o timeouts; revisar esas tareas porque pueden estar ciclando y sosteniendo CPU.".to_string(),
        );
    }

    if load_1m > 4.0 {
        recommendations.push(
            "Aunque ya haya swap, el control-plane sigue contribuyendo a un load por encima de los 4 vCPU. Separar Coolify del workload o mover sitios sensibles sigue siendo una opcion realista.".to_string(),
        );
    }

    if recommendations.is_empty() {
        recommendations.push(
            "El control-plane no aparece como hotspot dominante en esta toma; repetir el muestreo cuando el load vuelva a subir.".to_string(),
        );
    }

    recommendations
}

fn queue_lengths_are_zero(summary: &str) -> bool {
    summary
        .split("len=")
        .skip(1)
        .all(|segment| segment.chars().next().is_some_and(|ch| ch == '0'))
}
