/* [124A-IMAGE404] Gestión de volúmenes persistentes.
 *
 * Coolify normaliza bind mount paths a named volumes en su API interna.
 * Esto causa que las imágenes/uploads desaparezcan después de un redeploy
 * o restart iniciado desde Coolify (UI o API), porque Coolify reescribe
 * el compose en disco con su versión procesada que usa named volumes.
 *
 * Este módulo proporciona funciones para forzar bind mounts en el compose
 * en disco, garantizando que docker compose build/up siempre use el path
 * persistente del host.
 *
 * Gotcha: Coolify procesa compose volumes así:
 *   raw: 'uploads_data:/app/uploads' → processed: 'UUID_uploads-data:/app/uploads'
 *   raw: '/data/uploads/studio:/app/uploads' → processed: 'UUID_uploads-data:/app/uploads'
 * Ambos formatos se normalizan al mismo named volume. No hay forma de evitarlo
 * via API. La solución es parchear el archivo en disco después de que Coolify escriba. */

use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

/* Forzar bind mount para /app/uploads en el compose en disco.
 *
 * Busca cualquier volumen que mapee a /app/uploads (sea named volume o bind mount
 * incorrecto) y lo reemplaza con el bind mount persistente del host.
 *
 * El patrón sed usa [^[:space:]]+ para cubrir todos los formatos de quoting:
 * - Sin comillas: UUID_uploads-data:/app/uploads
 * - Comillas simples: 'UUID_uploads-data:/app/uploads'
 * - Comillas dobles: "UUID_uploads-data:/app/uploads"
 *
 * Coolify puede escribir el compose en disco con o sin comillas dependiendo
 * de la versión y el contexto. El patrón anterior solo cubría comillas simples.
 *
 * Fallback: si no existe ninguna línea :/app/uploads, inserta el bind mount
 * después de la primera sección volumes: del compose (usando awk). */
pub async fn ensure_uploads_bind_mount(
    ssh: &SshClient,
    service_dir: &str,
    site_name: &str,
) -> std::result::Result<(), CoolifyError> {
    let host_path = format!("/data/uploads/{}", site_name);
    let compose_file = format!("{}/docker-compose.yml", service_dir);

    /* Paso 1: sed con patrón amplio que cubre cualquier formato de quoting */
    let fix_cmd = format!(
        "sed -i -E \"s|[^[:space:]]+:/app/uploads[^[:space:]]*|'{host_path}:/app/uploads'|g\" {compose_file}",
    );
    ssh.execute(&fix_cmd).await?;

    let verify = ssh
        .execute(&format!(
            "grep -c '{}:/app/uploads' {}",
            host_path, compose_file
        ))
        .await?;
    let count: i32 = verify.stdout.trim().parse().unwrap_or(0);
    if count > 0 {
        println!("      Bind mount forzado: {}:/app/uploads", host_path);
        return Ok(());
    }

    /* Paso 2 (fallback): no existía ninguna línea :/app/uploads en el compose.
     * Insertar el bind mount después de la primera sección volumes: usando awk.
     * \047 es el octal de comilla simple para evitar problemas de quoting en bash. */
    println!("      No se encontró volumen :/app/uploads — insertando...");
    let fallback_cmd = format!(
        "awk -v bind=\"{host_path}:/app/uploads\" \
         'BEGIN{{f=0}}/volumes:/ && !f{{print; print \"      - \\047\" bind \"\\047\"; f=1; next}}1' \
         {compose_file} > {compose_file}.tmp && mv {compose_file}.tmp {compose_file}",
    );
    ssh.execute(&fallback_cmd).await?;

    /* Re-verificar después del fallback */
    let verify2 = ssh
        .execute(&format!(
            "grep -c '{}:/app/uploads' {}",
            host_path, compose_file
        ))
        .await?;
    let count2: i32 = verify2.stdout.trim().parse().unwrap_or(0);
    if count2 > 0 {
        println!("      Bind mount insertado: {}:/app/uploads", host_path);
        Ok(())
    } else {
        /* Debug: mostrar líneas relevantes para diagnóstico remoto */
        let debug = ssh
            .execute(&format!(
                "grep -n 'volumes\\|uploads\\|/app/' {} || echo 'Sin coincidencias'",
                compose_file
            ))
            .await?;
        Err(CoolifyError::Validation(format!(
            "No se pudo aplicar bind mount para uploads en {}.\n\
             Líneas relevantes del compose:\n{}\n\
             Verificar manualmente el compose en disco.",
            compose_file, debug.stdout
        )))
    }
}

/* Preparar directorio de uploads en el host.
 * Crea la estructura de subdirectorios y establece permisos.
 * chmod 777 porque el contenedor puede correr con UID variable (appuser). */
pub async fn ensure_uploads_host_dir(
    ssh: &SshClient,
    site_name: &str,
) -> std::result::Result<String, CoolifyError> {
    let uploads_host_dir = format!("/data/uploads/{}", site_name);
    ssh.execute(&format!(
        "mkdir -p {uploads_host_dir}/content {uploads_host_dir}/deliverables && chmod -R 777 {uploads_host_dir}"
    ))
    .await?;
    Ok(uploads_host_dir)
}
