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
 * El patrón sed busca dentro de comillas simples: 'ANYTHING:/app/uploads'
 * Esto cubre:
 * - Named volumes Coolify: 'UUID_uploads-data:/app/uploads'
 * - Bind mounts incorrectos: '/wrong/path:/app/uploads'
 * - Bind mounts correctos: '/data/uploads/studio:/app/uploads' (idempotente)
 *
 * Retorna error si después del patch no se encuentra el bind mount esperado. */
pub async fn ensure_uploads_bind_mount(
    ssh: &SshClient,
    service_dir: &str,
    site_name: &str,
) -> std::result::Result<(), CoolifyError> {
    let host_path = format!("/data/uploads/{}", site_name);
    let compose_file = format!("{}/docker-compose.yml", service_dir);

    let fix_cmd = format!(
        "sed -i \"s|'[^']*:/app/uploads'|'{}:/app/uploads'|g\" {}",
        host_path, compose_file,
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
        Ok(())
    } else {
        Err(CoolifyError::Validation(format!(
            "No se pudo aplicar bind mount para uploads en {}. \
             Verificar manualmente el compose en disco.",
            compose_file
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
