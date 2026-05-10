/*
 * Tipos compartidos que mapean a los structs Rust de api::types.
 */

export interface SitioResumen {
    name: string;
    domain: string;
    target: string;
    stack_uuid: string;
    template: string;
}

export interface MinecraftResumen {
    name: string;
    memory: string;
    max_players: number;
}

export interface RespuestaSitios {
    sites: SitioResumen[];
    minecraft: MinecraftResumen[];
}

export interface TargetResumen {
    name: string;
    host: string;
    user: string;
    coolify_url: string;
    site_count: number;
}

export interface RespuestaTargets {
    default_target: string;
    config_path: string;
    targets: TargetResumen[];
}

export interface CrearSitioRequest {
    name: string;
    domain: string;
    template: string;
    target: string;
    skipTheme?: boolean;
    skipCache?: boolean;
}

export interface RespuestaSalud {
    site_name: string;
    url: string;
    http_ok: boolean;
    app_ok: boolean;
    fatal_log_detected: boolean;
    status_code: number | null;
    healthy: boolean;
    details: string[];
}

export interface ResumenBackup {
    backup_id: string;
    tier: string;
    status: string;
    created_at: string;
    label: string | null;
    artifact_count: number;
}

export interface RespuestaBackups {
    site_name: string;
    backups: ResumenBackup[];
}

export interface ResumenBackupGlobal extends ResumenBackup {
    site_name: string;
    domain: string;
    target: string;
    template: string;
}

export interface ErrorBackupsGlobal {
    site_name: string;
    message: string;
}

export interface RespuestaBackupsGlobal {
    backups: ResumenBackupGlobal[];
    errors: ErrorBackupsGlobal[];
}

export interface RespuestaAuditoria {
    target: string;
    load_average: string;
    memory_summary: string;
    disk_summary: string;
    docker_summary: string;
    security_summary: string;
    recommendations: string[];
    load_1m: number | null;
    load_5m: number | null;
    load_15m: number | null;
    memory_used_mb: number | null;
    memory_free_mb: number | null;
    memory_total_mb: number | null;
    disk_use_percent: number | null;
}

export interface MetricaContenedor {
    name: string;
    cpu_percent: number;
    memory_usage: string;
    memory_percent: number;
    memory_used_bytes: number;
    memory_limit_bytes: number;
}

export interface MetricaDespliegue {
    site_name: string;
    target: string;
    status: string;
    total_cpu_percent: number;
    memory_used_bytes: number;
    memory_limit_bytes: number;
    memory_percent: number;
    containers: MetricaContenedor[];
    updated_at: string;
}

export interface RespuestaMetricasDespliegue {
    generated_at: string;
    metrics: MetricaDespliegue[];
}

export interface RespuestaLogs {
    site_name: string;
    container_target: string;
    lines: number;
    content: string;
    stderr: string;
    exit_code: number;
}

export interface ResultadoOperacion {
    success: boolean;
    message: string;
    details: string | null;
}
