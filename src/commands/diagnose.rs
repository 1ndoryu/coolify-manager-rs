/*
 * [276A-1] Comando: diagnose
 * Diagnostico completo de un sitio via SSH: contenedores, discos, BD, bind mounts, logs.
 * NO modifica nada — solo recolecta y reporta.
 *
 * Uso: coolify-manager diagnose --name kamples
 *      coolify-manager diagnose --name kamples --json
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    json_output: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;

    let stack_uuid = match &site.stack_uuid {
        Some(u) => u.clone(),
        None => {
            return Err(CoolifyError::Validation(format!(
                "El sitio '{site_name}' no tiene stackUuid configurado"
            )));
        }
    };

    let target = settings.resolve_site_target(site)?;
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;

    /* ── Recolectar datos en paralelo (orden secuencial SSH, pero agrupado) ── */

    let header = format!(
        "═══ Diagnóstico: {site_name} ═══\nUUID: {stack_uuid}\nTemplate: {}\nTarget: {}\n",
        site.template,
        target.name,
    );

    /* 1. Contenedores del stack */
    let containers = ssh_exec_or(
        &ssh,
        &format!(
            "docker ps -a --filter label=coolify.stack-uuid={stack_uuid} \
             --format 'table {{{{.Names}}}}\t{{{{.Status}}}}\t{{{{.Image}}}}'"
        ),
        "(sin contenedores — stack no deployado o uuid incorrecto)".to_string(),
    )
    .await;

    /* 2. Docker system df resumido */
    let docker_df = ssh_exec_or(
        &ssh,
        "docker system df --format 'table {{.Type}}\t{{.TotalCount}}\t{{.Size}}\t{{.Reclaimable}}' 2>/dev/null || echo '(docker system df no disponible)'",
        "(error al obtener docker system df)".to_string(),
    )
    .await;

    /* 3. Volumenes Docker del stack + tamaños */
    let volumes = ssh_exec_or(
        &ssh,
        &format!(
            "volumes=$(docker volume ls --filter label=coolify.stack-uuid={stack_uuid} --format '{{{{.Name}}}}') && \
             if [ -z \"$volumes\" ]; then \
               volumes=$(docker volume ls --filter name={stack_uuid} --format '{{{{.Name}}}}'); \
             fi && \
             if [ -z \"$volumes\" ]; then \
               echo '(no se encontraron volumenes con label coolify.stack-uuid)'; \
             else \
               echo \"$volumes\" | while read v; do \
                 size=$(du -sh /var/lib/docker/volumes/$v/_data/ 2>/dev/null | cut -f1); \
                 echo \"  $v  [$size]\"; \
               done; \
             fi"
        ),
        "(error al listar volumenes)".to_string(),
    )
    .await;

    /* 4. BD PostgreSQL — tablas, registros, tamaño */
    let pg_container = ssh_exec_or(
        &ssh,
        &format!(
            "docker ps --format '{{{{.Names}}}}' | grep -i '{stack_uuid}' | grep -i postgres | head -1"
        ),
        String::new(),
    )
    .await;
    let pg_container = pg_container.trim();

    let pg_info = if pg_container.is_empty() {
        String::from("  (no se encontro contenedor postgres para este stack)")
    } else {
        let pg_db = ssh_exec_or(
            &ssh,
            &format!(
                "docker exec {pg_container} psql -U postgres -t -A -c \"SELECT datname FROM pg_database WHERE datistemplate=false\" 2>/dev/null | head -5"
            ),
            String::new(),
        ).await;
        let pg_db = pg_db.trim();

        if pg_db.is_empty() {
            String::from("  (contenedor postgres encontrado pero sin databases accesibles)")
        } else {
            let pg_tables = ssh_exec_or(
                &ssh,
                &format!(
                    "docker exec {pg_container} psql -d {pg_db} -t -A -c \"SELECT count(*) FROM information_schema.tables WHERE table_schema='public'\" 2>/dev/null"
                ),
                String::new(),
            ).await;
            let pg_rows = ssh_exec_or(
                &ssh,
                &format!(
                    "docker exec {pg_container} psql -d {pg_db} -t -A -c \"SELECT sum(n_live_tup)::bigint FROM pg_stat_user_tables\" 2>/dev/null"
                ),
                String::new(),
            ).await;
            let pg_size = ssh_exec_or(
                &ssh,
                &format!(
                    "docker exec {pg_container} psql -d {pg_db} -t -A -c \"SELECT pg_size_pretty(pg_database_size(current_database()))\" 2>/dev/null"
                ),
                String::new(),
            ).await;

            format!(
                "  Contenedor: {pg_container}\n  Base de datos: {pg_db}\n  Tablas: {pg_tables}\n  Registros totales: {pg_rows}\n  Tamaño: {pg_size}"
            )
        }
    };

    /* 5. BD MySQL/MariaDB (WordPress) — si existe */
    let mariadb_container = ssh_exec_or(
        &ssh,
        &format!(
            "docker ps --format '{{{{.Names}}}}' | grep -i '{stack_uuid}' | grep -iE 'mariadb|mysql' | head -1"
        ),
        String::new(),
    )
    .await;
    let mariadb_container = mariadb_container.trim();

    let mariadb_info = if mariadb_container.is_empty() {
        String::from("  (no se encontro contenedor mariadb/mysql para este stack)")
    } else {
        let mysql_dbs = ssh_exec_or(
            &ssh,
            &format!(
                "docker exec {mariadb_container} mysql -e \"SHOW DATABASES\" 2>/dev/null | tail -n +2 | head -10"
            ),
            String::new(),
        ).await;
        let mysql_dbs = mysql_dbs.trim();

        if mysql_dbs.is_empty() {
            String::from("  (contenedor mariadb sin databases accesibles)")
        } else {
            let mysql_wp = ssh_exec_or(
                &ssh,
                &format!(
                    "docker exec {mariadb_container} mysql -e \"SELECT table_schema, COUNT(*) AS tables, ROUND(SUM(data_length+index_length)/1024/1024, 1) AS size_mb FROM information_schema.tables WHERE table_schema NOT IN ('information_schema','performance_schema','mysql') GROUP BY table_schema\" 2>/dev/null | tail -n +2 | head -10"
                ),
                String::new(),
            ).await;

            format!("  Contenedor: {mariadb_container}\n  Bases de datos:\n{mysql_dbs}\n\n  Tamaños:\n{mysql_wp}")
        }
    };

    /* 6. Docker compose on-disk (primeras 80 lineas) */
    let compose = ssh_exec_or(
        &ssh,
        &format!("cat /data/coolify/services/{stack_uuid}/docker-compose.yml 2>/dev/null | head -80 || echo '(no existe docker-compose.yml on-disk)'"),
        "(error al leer docker-compose.yml)".to_string(),
    )
    .await;

    /* 7. Bind mounts del stack */
    let bind_mounts = ssh_exec_or(
        &ssh,
        &format!(
            "echo '=== docker inspect volumes ===' && \
             docker inspect $(docker ps -a --filter label=coolify.stack-uuid={stack_uuid} --format '{{{{.ID}}}}' | head -1) 2>/dev/null | \
             grep -A5 '\"Type\":\"bind\"' | head -40 && \
             echo '' && \
             echo '=== /data/uploads/ ===' && \
             ls -la /data/uploads/ 2>/dev/null | grep -i '{site_name}' | head -5 && \
             echo '' && \
             echo '=== du -sh /data/uploads/{site_name} ===' && \
             du -sh /data/uploads/{site_name} 2>/dev/null || echo '(no existe bind mount /data/uploads/{site_name})'",
            stack_uuid = stack_uuid,
            site_name = site_name,
        ),
        "(error al inspeccionar bind mounts)".to_string(),
    )
    .await;

    /* 8. Logs del contenedor principal (primer container no-BD) */
    let main_container = ssh_exec_or(
        &ssh,
        &format!(
            "docker ps -a --filter label=coolify.stack-uuid={stack_uuid} --format '{{{{.Names}}}}' | grep -viE 'mariadb|postgres|redis|db-' | head -1"
        ),
        String::new(),
    )
    .await;
    let main_container = main_container.trim();

    let logs = if main_container.is_empty() {
        String::from("  (no se encontro contenedor principal del stack)")
    } else {
        ssh_exec_or(
            &ssh,
            &format!(
                "docker logs {main_container} --tail 30 2>&1 | head -40 || echo '(no logs disponibles)'"
            ),
            String::new(),
        ).await
    };

    /* 9. Docker inspect — tamaños de capas */
    let first_container_id = ssh_exec_or(
        &ssh,
        &format!(
            "docker ps -a --filter label=coolify.stack-uuid={stack_uuid} --format '{{{{.ID}}}}' | head -1"
        ),
        String::new(),
    )
    .await;
    let first_container_id = first_container_id.trim();

    let container_sizes = if first_container_id.is_empty() {
        String::from("  (no hay contenedores para inspeccionar)")
    } else {
        ssh_exec_or(
            &ssh,
            &format!(
                "docker inspect {first_container_id} --format 'RootFS: {{{{.SizeRootFs}}}} bytes  Rw: {{{{.SizeRw}}}} bytes' 2>/dev/null"
            ),
            String::new(),
        ).await
    };

    /* 10. Estado del stack via Coolify API */
    let coolify_status = ssh_exec_or(
        &ssh,
        &format!(
            "curl -sf --max-time 10 \
              -H 'Authorization: Bearer {api_token}' \
              '{base_url}/api/v1/services/{stack_uuid}' 2>/dev/null | \
              python3 -c \"import sys,json; d=json.load(sys.stdin); print(f'Status: {{d.get(\\\"status\\\",\\\"?\\\")}}  Name: {{d.get(\\\"name\\\",\\\"?\\\")}}  FQDN: {{d.get(\\\"fqdn\\\",\\\"?\\\")}}')\" 2>/dev/null || echo '(Coolify API no accesible desde este VPS)'",
            api_token = settings.coolify.api_token,
            base_url = settings.coolify.base_url.trim_end_matches('/'),
        ),
        "(error al consultar Coolify API)".to_string(),
    )
    .await;

    /* ── 11. Inspección profunda de volúmenes ── */
    /* Cuenta archivos, muestra samples y verifica si hay datos reales */

    let vol_base = "/var/lib/docker/volumes";

    let uploads_deep = ssh_exec_or(
        &ssh,
        &format!(
            "echo '=== uploads-data: conteo total ===' && \
             find {vol_base}/mo4so4440c488g8woow4cow0_uploads-data/_data/ -type f 2>/dev/null | wc -l && \
             echo '' && \
             echo '=== uploads-data: top 20 directorios por tamaño ===' && \
             du -sh {vol_base}/mo4so4440c488g8woow4cow0_uploads-data/_data/*/ 2>/dev/null | sort -rh | head -20 && \
             echo '' && \
             echo '=== uploads-data: archivos recientes (últimos 5 modificados) ===' && \
             find {vol_base}/mo4so4440c488g8woow4cow0_uploads-data/_data/ -type f -printf '%T@ %p\\n' 2>/dev/null | sort -rn | head -5 && \
             echo '' && \
             echo '=== uploads-data: sample de estructura ===' && \
             ls -la {vol_base}/mo4so4440c488g8woow4cow0_uploads-data/_data/ 2>/dev/null | head -20"
        ),
        "(error al inspeccionar uploads-data)".to_string(),
    )
    .await;

    let pg_deep = ssh_exec_or(
        &ssh,
        &format!(
            "echo '=== pg-data: estructura PGDATA (primer nivel) ===' && \
             ls -la {vol_base}/mo4so4440c488g8woow4cow0_pg-data/_data/ 2>/dev/null | head -30 && \
             echo '' && \
             echo '=== pg-data: databases (subdirectorios) ===' && \
             ls -d {vol_base}/mo4so4440c488g8woow4cow0_pg-data/_data/base/*/ 2>/dev/null | wc -l && \
             echo '' && \
             echo '=== pg-data: tamaño total de archivos de datos ===' && \
             du -sh {vol_base}/mo4so4440c488g8woow4cow0_pg-data/_data/base/ 2>/dev/null && \
             echo '' && \
             echo '=== pg-data: pg_stat (indica si fue inicializado) ===' && \
             ls {vol_base}/mo4so4440c488g8woow4cow0_pg-data/_data/pg_stat/ 2>/dev/null | wc -l"
        ),
        "(error al inspeccionar pg-data)".to_string(),
    )
    .await;

    let mariadb_deep = ssh_exec_or(
        &ssh,
        &format!(
            "echo '=== db-data: estructura MariaDB ===' && \
             ls -la {vol_base}/mo4so4440c488g8woow4cow0_db-data/_data/ 2>/dev/null | head -30 && \
             echo '' && \
             echo '=== db-data: bases de datos (directorios) ===' && \
             ls -d {vol_base}/mo4so4440c488g8woow4cow0_db-data/_data/*/ 2>/dev/null && \
             echo '' && \
             echo '=== db-data: archivos .ibd (tablas InnoDB) ===' && \
             find {vol_base}/mo4so4440c488g8woow4cow0_db-data/_data/ -name '*.ibd' 2>/dev/null | wc -l && \
             echo '' && \
             echo '=== db-data: tamaño total ===' && \
             du -sh {vol_base}/mo4so4440c488g8woow4cow0_db-data/_data/ 2>/dev/null"
        ),
        "(error al inspeccionar db-data)".to_string(),
    )
    .await;

    /* wp-data: tamaño y estructura */
    let wp_deep = ssh_exec_or(
        &ssh,
        &format!(
            "echo '=== wordpress-data: conteo archivos ===' && \
             find {vol_base}/mo4so4440c488g8woow4cow0_wordpress-data/_data/ -type f 2>/dev/null | wc -l && \
             echo '' && \
             echo '=== wordpress-data: estructura raiz ===' && \
             ls -la {vol_base}/mo4so4440c488g8woow4cow0_wordpress-data/_data/ 2>/dev/null | head -20 && \
             echo '' && \
             echo '=== wordpress-data: wp-content/uploads ===' && \
             ls -la {vol_base}/mo4so4440c488g8woow4cow0_wordpress-data/_data/wp-content/uploads/ 2>/dev/null | head -20"
        ),
        "(error al inspeccionar wordpress-data)".to_string(),
    )
    .await;

    /* Ver si el volumen largo (mo4so..._mo4so...) tiene datos duplicados o diferentes */
    let long_uploads = ssh_exec_or(
        &ssh,
        &format!(
            "echo '=== volumen largo uploads-data ===' && \
             du -sh {vol_base}/mo4so4440c488g8woow4cow0_mo4so4440c488g8woow4cow0-uploads-data/_data/ 2>/dev/null && \
             ls -la {vol_base}/mo4so4440c488g8woow4cow0_mo4so4440c488g8woow4cow0-uploads-data/_data/ 2>/dev/null | head -10"
        ),
        "(no hay volumen largo uploads)".to_string(),
    )
    .await;

    /* ── 12. Sonda PostgreSQL: docker run --rm para verificar el volumen desde dentro de PG ── */
    /* Monta el volumen como read-only e intenta listar, detectar si PG puede leerlo y consultar databases.
     * El container se elimina automaticamente (--rm). No modifica nada (read-only). */
    let pg_probe = ssh_exec_or(
        &ssh,
        &format!(
            "echo '=== Sonda PostgreSQL (docker run --rm, RO) ===' && \
             docker run --rm \
               -v {stack_uuid}_pg-data:/var/lib/postgresql/data:ro \
               --entrypoint bash \
               postgres:18-alpine \
               -c '
             echo \"--- 1. PGDATA desde dentro del container ---\"
             ls -la /var/lib/postgresql/data/
             echo \"\"
             echo \"--- 2. Recursivo (primer nivel de subdirectorios) ---\"
             for d in /var/lib/postgresql/data/*/; do
               name=$(basename \"$d\")
               count=$(find \"$d\" -type f 2>/dev/null | wc -l)
               size=$(du -sh \"$d\" 2>/dev/null | cut -f1)
               echo \"  $name  (files=$count, size=$size)\"
             done
             echo \"\"
             echo \"--- 3. Verificacion de estructura estandar PG ---\"
             for p in PG_VERSION base global pg_wal pg_xact pg_stat postgresql.conf pg_hba.conf; do
               if [ -e \"/var/lib/postgresql/data/$p\" ]; then
                 echo \"  PRESENTE: $p\"
               else
                 echo \"  AUSENTE: $p\"
               fi
             done
             echo \"\"
             echo \"--- 4. Intentando arrancar PG (postgres --single) ---\"
             if [ -f /var/lib/postgresql/data/global/pg_control ]; then
               echo \"  global/pg_control EXISTE - BD inicializada\"
               pg_controldata /var/lib/postgresql/data/ 2>&1 | head -20
             else
               echo \"  global/pg_control NO EXISTE - BD nunca inicializada o corrupta\"
             fi
             ' 2>&1 || echo '(ERROR: docker run fallo - posiblemente la imagen postgres:18-alpine no esta disponible localmente)'"
        ),
        "(no se pudo ejecutar la sonda PostgreSQL)".to_string(),
    )
    .await;

    /* ── Ensamblar y mostrar el reporte ── */

    if json_output {
        let report = serde_json::json!({
            "site": site_name,
            "stack_uuid": stack_uuid,
            "template": format!("{}", site.template),
            "containers": containers.trim(),
            "docker_df": docker_df.trim(),
            "volumes": volumes.trim(),
            "postgresql": pg_info.trim(),
            "mariadb": mariadb_info.trim(),
            "compose": compose.trim(),
            "bind_mounts": bind_mounts.trim(),
            "logs": logs.trim(),
            "container_sizes": container_sizes.trim(),
            "coolify_status": coolify_status.trim(),
            "deep_uploads": uploads_deep.trim(),
            "deep_postgres": pg_deep.trim(),
            "deep_mariadb": mariadb_deep.trim(),
            "deep_wordpress": wp_deep.trim(),
            "deep_long_uploads": long_uploads.trim(),
            "pg_probe": pg_probe.trim(),
        });
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else {
        println!("{header}");
        println!("── Contenedores del stack ──\n{containers}");
        println!("\n── Docker system df ──\n{docker_df}");
        println!("\n── Volúmenes Docker ──\n{volumes}");
        println!("\n── Base de datos PostgreSQL ──\n{pg_info}");
        println!("\n── Base de datos MySQL/MariaDB ──\n{mariadb_info}");
        println!("\n── Docker Compose on-disk (primeras 80 líneas) ──\n{compose}");
        println!("\n── Bind mounts ──\n{bind_mounts}");
        println!("\n── Logs del contenedor principal (últimas 30) ──\n{logs}");
        println!("\n── Tamaños de capas Docker ──\n{container_sizes}");
        println!("\n── Estado Coolify API ──\n{coolify_status}");
        println!("\n── Inspección profunda: uploads-data ──\n{uploads_deep}");
        println!("\n── Inspección profunda: PostgreSQL ──\n{pg_deep}");
        println!("\n── Inspección profunda: MariaDB ──\n{mariadb_deep}");
        println!("\n── Inspección profunda: WordPress ──\n{wp_deep}");
        println!("\n── Volumen largo uploads-data ──\n{long_uploads}");
        println!("\n── Sonda PostgreSQL (contenedor temporal) ──\n{pg_probe}");
        println!("\n═══ Fin del diagnóstico ── {site_name} ═══");
    }

    Ok(())
}

/// Ejecuta un comando SSH y devuelve stdout + stderr.
/// Si falla, devuelve el fallback string proporcionado.
async fn ssh_exec_or(ssh: &SshClient, cmd: &str, fallback: String) -> String {
    match ssh.execute(cmd).await {
        Ok(output) => {
            let combined = format!("{}{}", output.stdout, output.stderr);
            if combined.trim().is_empty() {
                fallback
            } else {
                combined
            }
        }
        Err(e) => format!("  (error SSH: {e})"),
    }
}
