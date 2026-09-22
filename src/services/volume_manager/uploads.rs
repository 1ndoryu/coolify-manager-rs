/* Bind mount de uploads: forzar, preparar, fusionar y verificar (119A-5 split volume_manager). */

use super::compose_remoto::shell_single_quote;
use super::texto::mount_points_app_uploads_to;
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
    compose_service: &str,
) -> std::result::Result<(), CoolifyError> {
    let host_path = format!("/data/uploads/{}", site_name);
    let compose_file = format!("{}/docker-compose.yml", service_dir);
    let bind_value = format!("{host_path}:/app/uploads");

    /* [E7-fix] Usar Python para manipulación YAML confiable.
     * El awk anterior insertaba el bind mount en el PRIMER volumes: encontrado
     * (que suele ser postgres, no app). Python rastrea el bloque del servicio
     * correcto y maneja ambos casos: volumes existente o no. */
    let py_script = r#"import sys
cf, svc, bind = sys.argv[1], sys.argv[2], sys.argv[3]
with open(cf) as f:
    lines = f.readlines()
# 1) Eliminar TODAS las líneas /app/uploads existentes (pueden estar en el servicio equivocado)
lines = [l for l in lines if '/app/uploads' not in l]
# 2) Encontrar el bloque del servicio destino
in_svc = False
svc_start = None
insert_at = len(lines)
has_volumes = False
for i, line in enumerate(lines):
    s = line.rstrip()
    if s == '  ' + svc + ':':
        in_svc = True
        svc_start = i
        continue
    if in_svc and line.startswith('  ') and not line.startswith('    ') and s:
        insert_at = i
        in_svc = False
        break
    if in_svc and s.strip() == 'volumes:' and line.startswith('    '):
        has_volumes = True
if svc_start is None:
    print('Service ' + svc + ' not found')
    sys.exit(1)
if has_volumes:
    for i in range(svc_start, insert_at):
        if lines[i].strip() == 'volumes:' and lines[i].startswith('    '):
            j = i + 1
            while j < insert_at and lines[j].startswith('      '):
                j += 1
            lines.insert(j, "      - '" + bind + "'\n")
            break
else:
    lines.insert(insert_at, "    volumes:\n      - '" + bind + "'\n")
with open(cf, 'w') as f:
    f.writelines(lines)
"#;
    let temp_script = format!("/tmp/cm-bind-{}.py", std::process::id());
    /* Escribir script como base64 para evitar problemas de quoting */
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(py_script.as_bytes());
    ssh.execute(&format!("echo {encoded} | base64 -d > {temp_script}"))
        .await?;

    let result = ssh
        .execute(&format!(
            "python3 {temp_script} {cf} {svc} {bind}",
            cf = shell_single_quote(&compose_file),
            svc = shell_single_quote(compose_service),
            bind = shell_single_quote(&bind_value),
        ))
        .await?;
    let _ = ssh.execute(&format!("rm -f {temp_script}")).await?;

    if !result.success() {
        return Err(CoolifyError::Validation(format!(
            "Error al insertar bind mount: {}",
            result.stderr.trim()
        )));
    }

    /* Verificar */
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

/* [235A-5] Cuando Coolify reescribe /app/uploads como named volume, el bind real
 * puede seguir teniendo las imágenes antiguas mientras el volumen equivocado acumula
 * subidas nuevas. Antes de recrear el contenedor, fusionar el contenido actual en el
 * bind host con cp -n para preservar ambas fuentes sin sobrescribir. */
pub async fn merge_current_uploads_into_host_bind(
    ssh: &SshClient,
    service_dir: &str,
    app_service_name: &str,
    site_name: &str,
) -> std::result::Result<(), CoolifyError> {
    let host_path = format!("/data/uploads/{}", site_name);
    let container_lookup = ssh
        .execute(&format!(
            "cd {} && docker compose ps -q {} 2>/dev/null || true",
            service_dir, app_service_name
        ))
        .await?;
    let container_id = container_lookup.stdout.trim();
    if container_id.is_empty() {
        return Ok(());
    }

    let mounts = ssh
        .execute(&format!(
            "docker inspect {} --format '{{{{range .Mounts}}}}{{{{println .Destination \"|\" .Type \"|\" .Source}}}}{{{{end}}}}'",
            container_id
        ))
        .await?;

    if mounts
        .stdout
        .lines()
        .any(|line| mount_points_app_uploads_to(line, &host_path))
    {
        return Ok(());
    }

    let file_count = ssh
        .execute(&format!(
            "docker exec {} sh -c 'find /app/uploads -type f 2>/dev/null | wc -l'",
            container_id
        ))
        .await?;
    let count: u64 = file_count.stdout.trim().parse().unwrap_or(0);
    if count == 0 {
        return Ok(());
    }

    let merge_cmd = format!(
        "tmp=$(mktemp -d /tmp/cm-uploads-merge.XXXXXX) \
         && docker cp {container_id}:/app/uploads/. \"$tmp/\" \
         && mkdir -p {host_path} \
         && cp -an \"$tmp/.\" {host_path}/ \
         && chmod -R 777 {host_path} \
         && rm -rf \"$tmp\" \
         && echo MERGED",
    );
    let merge = ssh.execute(&merge_cmd).await?;
    if !merge.success() || !merge.stdout.contains("MERGED") {
        return Err(CoolifyError::Validation(format!(
            "No se pudo fusionar uploads desde el contenedor actual: {}{}",
            merge.stdout.trim(),
            merge.stderr.trim()
        )));
    }

    println!(
        "      Uploads del volumen actual fusionados en {} ({} archivos, sin sobrescribir).",
        host_path, count
    );
    Ok(())
}

/* [045A-GUARDRAILS] Después de cualquier restart/swap, validar el mount efectivo
 * del contenedor. Healthcheck OK no basta: el contenedor puede estar sano pero
 * usando un named volume vacío en /app/uploads. */
pub async fn verify_runtime_uploads_bind_mount(
    ssh: &SshClient,
    service_dir: &str,
    app_service_name: &str,
    site_name: &str,
) -> std::result::Result<(), CoolifyError> {
    let host_path = format!("/data/uploads/{}", site_name);
    let container_lookup = ssh
        .execute(&format!(
            "cd {} && docker compose ps -q {} 2>/dev/null || true",
            service_dir, app_service_name
        ))
        .await?;
    let container_id = container_lookup.stdout.trim();
    if container_id.is_empty() {
        return Err(CoolifyError::Validation(format!(
            "No se encontró contenedor activo para '{}' al verificar uploads runtime",
            app_service_name
        )));
    }

    let mounts = ssh
        .execute(&format!(
            "docker inspect {} --format '{{{{range .Mounts}}}}{{{{println .Destination \"|\" .Type \"|\" .Source}}}}{{{{end}}}}'",
            container_id
        ))
        .await?;

    if mounts
        .stdout
        .lines()
        .any(|line| mount_points_app_uploads_to(line, &host_path))
    {
        println!(
            "      Runtime OK: /app/uploads usa bind mount {}",
            host_path
        );
        return Ok(());
    }

    Err(CoolifyError::Validation(format!(
        "ABORT: contenedor '{}' no usa bind mount '{}' en /app/uploads. Mounts detectados:\n{}",
        container_id,
        host_path,
        mounts.stdout.trim()
    )))
}
