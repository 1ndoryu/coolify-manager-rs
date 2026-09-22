/* Split 119A-5 de compare_manager.rs — resolución de dumps VPS/legacy.
 * Código verbatim del original; solo cambia visibilidad de ayudantes compartidos. */

use super::tipos::SideCreds;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;
use crate::services::compare::schema::DbEngine;

/// Construye el comando de búsqueda del último dump VPS (puro, testeable).
/// Cubre ambas variantes: `/data/backups/{uuid}` (Postgres) y
/// `/data/backups/mariadb-{uuid}` (WordPress/MariaDB).
pub(super) fn latest_dump_command(stack_uuid: &str) -> String {
    let base = format!("/data/backups/{stack_uuid}");
    let base_maria = format!("/data/backups/mariadb-{stack_uuid}");
    format!(
        "ls -1t {base}/daily/*.sql.gz {base}/weekly/*.sql.gz \
         {base_maria}/daily/*.sql.gz {base_maria}/weekly/*.sql.gz 2>/dev/null | head -1"
    )
}

/// Busca el último dump VPS disponible para un stack.
/// Nota: los stacks PostgreSQL se guardan en `/data/backups/{uuid}` y los
/// MariaDB/WordPress en `/data/backups/mariadb-{uuid}` (script backup-server.sh).
pub(super) async fn find_latest_vps_dump(
    ssh: &SshClient,
    stack_uuid: &str,
) -> std::result::Result<String, CoolifyError> {
    let res = ssh.execute(&latest_dump_command(stack_uuid)).await?;
    let path = res.stdout.trim().to_string();
    if path.is_empty() {
        return Err(CoolifyError::Validation(format!(
            "No hay dump VPS para stack {stack_uuid}. Ejecuta 'backup' primero."
        )));
    }
    Ok(path)
}

/// Resuelve `--dump legacy:<sufijo>` al tarball legacy del sitio en el VPS
/// (`/data/backups/coolify-manager/{sitio}/{daily,weekly,manual}/*<sufijo>*.tar.gz`).
/// Solo lectura sobre backups reales: jamás se borra ni modifica el tarball.
/// `sitio` y `sufijo` se validan (alnum+guion+bajo) antes de interpolar al shell.
pub(super) async fn resolve_legacy_dump(
    ssh: &SshClient,
    site_name: &str,
    suffix: &str,
) -> std::result::Result<String, CoolifyError> {
    let valido = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    };
    if !valido(site_name) || !valido(suffix) {
        return Err(CoolifyError::Validation(
            "legacy: solo admite sitio/sufijo alfanumérico con -/_".into(),
        ));
    }
    let cmd = format!(
        "ls -1t /data/backups/coolify-manager/{site_name}/daily/*{suffix}*.tar.gz \
         /data/backups/coolify-manager/{site_name}/weekly/*{suffix}*.tar.gz \
         /data/backups/coolify-manager/{site_name}/manual/*{suffix}*.tar.gz 2>/dev/null | head -1"
    );
    let res = ssh.execute(&cmd).await?;
    let path = res.stdout.trim().to_string();
    if path.is_empty() {
        return Err(CoolifyError::Validation(format!(
            "Sin tarball legacy '*{suffix}*' para sitio {site_name} en el VPS."
        )));
    }
    Ok(path)
}

/// Detecta la imagen Docker del motor para el contenedor temporal.
pub(super) fn detect_image_for(creds: &SideCreds) -> String {
    match creds.engine {
        DbEngine::Postgres => "postgres:16-alpine".to_string(),
        DbEngine::MariaDb => "mariadb:11".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mariadb_uuid_se_encuentra() {
        /* El comando debe cubrir ambas variantes de ruta o WP queda ciego */
        let cmd = latest_dump_command("owck8sww4ogk8gskgwcsk4w0");
        assert!(cmd.contains("/data/backups/owck8sww4ogk8gskgwcsk4w0/"));
        assert!(cmd.contains("/data/backups/mariadb-owck8sww4ogk8gskgwcsk4w0/"));
        assert!(cmd.contains("daily/*.sql.gz"));
        assert!(cmd.contains("weekly/*.sql.gz"));
    }
}
