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
