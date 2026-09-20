/*
 * delete-site — Borrado seguro y completo de un stack desechable.
 *
 * [119A-2] Capacidad exigida por el usuario para limpiar el test-site sin
 * tocar el dashboard ni arriesgar producción. Garantías por diseño:
 *   1. Resolución SOLO por nombre en settings.json: jamás se acepta un uuid
 *      crudo, así que un typo no puede apuntar a otro stack.
 *   2. Confirmación tipada: --confirm debe ser idéntico a --name.
 *   3. El service_dir se valida (solo [A-Za-z0-9-]) antes de cualquier `rm -rf`.
 *   4. `docker compose down --volumes` acotado al service_dir del stack.
 *   5. DELETE API con docker_cleanup=false y delete_connected_networks=false:
 *      jamás prune global ni redes compartidas (red `coolify` de Traefik).
 *   6. Verificación post-borrado: el stack debe dar 404 y los demás sitios
 *      de settings.json deben seguir existiendo en Coolify.
 *   7. El registro DNS se elimina si apunta a la VPS del stack (B4-4); si apunta
 *      a otra IP (migración) se conserva con aviso. Sin dnsConfig no hay nada
 *      que borrar.
 */

use crate::commands::deploy_service::rust_autoheal::{shell_single_quote, systemd_safe_name};
use crate::config::Settings;
use crate::services::dns_manager;
use crate::error::{ApiError, CoolifyError};
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::ssh_client::SshClient;

use std::path::Path;

/* [B4-3] Nombre de la unidad autoheal (misma regla que el instalador en
 * rust_autoheal: `cm-autoheal-{nombre saneado}`). */
pub(crate) fn nombre_unidad_autoheal(site_name: &str) -> String {
    format!("cm-autoheal-{}", systemd_safe_name(site_name))
}

/* [B4-3] Comando best-effort para retirar restos del stack en el host:
 * timer+service+script autoheal, imagen `{uuid}-app` y dir de uploads (solo si
 * está vacío — `rmdir` nunca borra datos). Todo con `|| true`: la limpieza no
 * puede hacer fallar el borrado. `unit` sale de `nombre_unidad_autoheal` y
 * `stack_uuid` ya pasó `ruta_service_dir_segura`, pero se entrecomilla igual. */
pub(crate) fn comando_limpieza_restos_host(
    unit: &str,
    stack_uuid: &str,
    site_name: &str,
) -> String {
    let unit_q = shell_single_quote(unit);
    let image_q = shell_single_quote(&format!("{stack_uuid}-app"));
    let uploads_q = shell_single_quote(&format!("/data/uploads/{site_name}"));
    format!(
        "systemctl disable --now {unit_q}.timer 2>/dev/null || true; \
         rm -f /etc/systemd/system/{unit_q}.service /etc/systemd/system/{unit_q}.timer \
         /usr/local/bin/{unit_q}.sh /tmp/{unit_q}.last; \
         systemctl daemon-reload 2>/dev/null || true; \
         docker rmi {image_q} 2>/dev/null || true; \
         rmdir {uploads_q} 2>/dev/null || true; \
         echo DELETE_SITE_RESTOS_OK"
    )
}

/* [119A-2] La confirmación tipada debe coincidir exacta con el objetivo. */
pub(crate) fn validar_confirmacion(site_name: &str, confirm: &str) -> Result<(), CoolifyError> {
    if confirm == site_name {
        Ok(())
    } else {
        Err(CoolifyError::Validation(format!(
            "Confirmación '{confirm}' no coincide con el sitio '{site_name}'. \
             Repite con --confirm {site_name} si realmente quieres borrarlo."
        )))
    }
}

/* [119A-2] Construye el service_dir solo si el uuid es seguro para `rm -rf`:
 * no vacío y únicamente [A-Za-z0-9-]. Cualquier otra cosa es Validation. */
pub(crate) fn ruta_service_dir_segura(stack_uuid: &str) -> Result<String, CoolifyError> {
    let seguro =
        !stack_uuid.is_empty() && stack_uuid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !seguro {
        return Err(CoolifyError::Validation(format!(
            "stackUuid con caracteres inseguros para borrado: '{stack_uuid}'"
        )));
    }
    Ok(format!("/data/coolify/services/{stack_uuid}"))
}

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    confirm: &str,
    dry_run: bool,
) -> std::result::Result<(), CoolifyError> {
    validar_confirmacion(site_name, confirm)?;

    let mut settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    let stack_uuid = site.stack_uuid.clone().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{site_name}' sin stackUuid configurado"))
    })?;
    let service_dir = ruta_service_dir_segura(&stack_uuid)?;
    let dominio = site.dominio.clone();
    /* Inventario de producción para la verificación post-borrado (D6). */
    let resto: Vec<(String, String)> = settings
        .sitios
        .iter()
        .filter(|s| s.nombre != site_name)
        .filter_map(|s| s.stack_uuid.clone().map(|u| (s.nombre.clone(), u)))
        .collect();
    let target = settings.resolve_site_target(site)?;

    println!("Borrado seguro del stack '{site_name}':");
    println!("  uuid:        {stack_uuid}");
    println!("  dominio:     {dominio}");
    println!("  service_dir: {service_dir}");
    println!("  otros sitios protegidos: {}", resto.len());

    if dry_run {
        println!("[dry-run] Se haría, en orden:");
        println!("  1. docker compose down --volumes --remove-orphans en {service_dir}");
        println!("  2. rm -rf {service_dir} + restos (timer autoheal, imagen, uploads vacíos)");
        println!("  3. DELETE /api/v1/services/{stack_uuid} (sin prune, sin redes compartidas)");
        println!("  4. Verificar 404 del stack + presencia de los {} otros sitios", resto.len());
        println!("  5. Borrar registros DNS que apunten a la VPS (se conservan si apuntan fuera)");
        println!("  6. Eliminar '{site_name}' de settings.json");
        return Ok(());
    }

    /* --- 1. Parada limpia acotada al service_dir del stack --- */
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;
    let down_cmd = format!(
        "if [ -d '{service_dir}' ]; then cd '{service_dir}' && \
         docker compose down --volumes --remove-orphans 2>&1; \
         else echo 'DELETE_SITE_SIN_DIR'; fi"
    );
    let down = ssh.execute(&down_cmd).await?;
    if down.stdout.contains("DELETE_SITE_SIN_DIR") {
        println!("  [1/6] Sin service_dir en host (stack nunca desplegado): se omite down.");
    } else {
        println!("  [1/6] compose down ejecutado:\n{}", down.stdout);
    }

    /* --- 2. Eliminar el directorio del servicio (ruta ya validada) --- */
    ssh.execute(&format!("rm -rf '{service_dir}'")).await?;
    let queda = ssh
        .execute(&format!("test -e '{service_dir}' && echo EXISTE || echo FUERA"))
        .await?;
    if queda.stdout.contains("EXISTE") {
        return Err(CoolifyError::Validation(format!(
            "service_dir {service_dir} sigue existiendo tras rm. Abortando antes del DELETE."
        )));
    }
    println!("  [2/6] service_dir eliminado del host.");

    /* --- 2b. [B4-3] Restos del stack: timer autoheal, imagen y uploads vacíos.
     * Best-effort: un resto que no se pueda retirar no bloquea el borrado. */
    let unit = nombre_unidad_autoheal(site_name);
    let restos = ssh
        .execute(&comando_limpieza_restos_host(&unit, &stack_uuid, site_name))
        .await?;
    if restos.stdout.contains("DELETE_SITE_RESTOS_OK") {
        println!("  [2/6] Restos retirados: timer {unit}, imagen {stack_uuid}-app, uploads vacíos.");
    } else {
        println!("  [2/6] ⚠ Limpieza de restos sin confirmar (no bloquea): {}", restos.stdout.trim());
    }

    /* --- 3. DELETE via Coolify API (banderas seguras en delete_stack) --- */
    let api = CoolifyApiClient::new(&target.coolify)?;
    match api.delete_stack(&stack_uuid).await {
        Ok(()) => println!("  [3/6] DELETE aceptado por Coolify API."),
        // Borrado idempotente: si el stack ya no existe (404), fue un DELETE
        // previo cuya cola ya se procesó. Se continúa a verificación (4a).
        Err(CoolifyError::Api(ApiError::HttpError { status: 404, .. })) => {
            println!("  [3/6] Stack ya ausente en Coolify (404: borrado previo procesado).");
        }
        Err(e) => return Err(e),
    }

    /* --- 4a. El stack debe haber desaparecido (404) --- */
    match api.get_service(&stack_uuid).await {
        Err(CoolifyError::Api(ApiError::HttpError { status: 404, .. })) => {
            println!("  [4/6] Stack verificado ausente (404).");
        }
        Ok(_) => {
            return Err(CoolifyError::Validation(format!(
                "El stack {stack_uuid} SIGUE existiendo tras el DELETE. \
                 Revisar en dashboard antes de eliminar '{site_name}' de settings."
            )));
        }
        Err(e) => return Err(e),
    }

    /* --- 4b. Los demás sitios deben seguir intactos --- */
    let servicios = api.get_services().await?;
    let uuids: Vec<&str> = servicios.iter().map(|s| s.uuid.as_str()).collect();
    for (nombre, uuid) in &resto {
        if !uuids.contains(&uuid.as_str()) {
            return Err(CoolifyError::Validation(format!(
                "CRÍTICO post-borrado: el sitio '{nombre}' ({uuid}) ya no aparece en Coolify. \
                 Investigar antes de continuar."
            )));
        }
    }
    println!("  [4/6] {} otros sitios verificados intactos.", resto.len());

    /* --- 5. [B4-4] Registros DNS que apunten a la VPS del stack.
     * Best-effort: el stack ya no existe; bloquear aquí dejaría settings
     * inconsistente. Si falla, se indica el comando exacto para reintentar. */
    match dns_manager::delete_site_dns(&settings, site, false).await {
        Ok(reporte) => {
            for accion in &reporte.actions {
                println!(
                    "  [5/6] DNS {} {} → {} [{}]",
                    accion.record_type, accion.record_name, accion.value, accion.action
                );
            }
            if reporte.actions.iter().all(|a| a.action == "absent") {
                println!("  [5/6] Sin registros DNS del sitio en la zona.");
            }
        }
        Err(error) => {
            let ayuda = match site.dns_config.as_ref() {
                Some(dns) => format!(
                    "delete-dns --provider {} --zone {} --name {dominio} --confirm {dominio}",
                    dns.provider, dns.zone, dominio = dominio
                ),
                None => format!(
                    "revisar el DNS de {dominio} a mano o con delete-dns --zone <zona> --name {dominio}"
                ),
            };
            println!("  [5/6] ⚠ DNS no eliminado (no bloquea): {error}");
            println!("         Reintentar con: {ayuda}");
        }
    }

    /* --- 6. Eliminar de settings.json local --- */
    settings.remove_site(site_name, config_path)?;
    println!("  [6/6] '{site_name}' eliminado de settings.json.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ruta_service_dir_segura, validar_confirmacion};

    #[test]
    fn confirmacion_debe_coincidir_con_objetivo() {
        assert!(validar_confirmacion("cm-test-119a2", "cm-test-119a2").is_ok());
        assert!(validar_confirmacion("cm-test-119a2", "cm-test-119a").is_err());
        assert!(validar_confirmacion("cm-test-119a2", "").is_err());
        /* Case-sensitive: protege contra typos silenciosos. */
        assert!(validar_confirmacion("cm-test-119a2", "CM-TEST-119A2").is_err());
    }

    #[test]
    fn service_dir_solo_uuid_seguro() {
        assert_eq!(
            ruta_service_dir_segura("abc-123-XYZ").unwrap(),
            "/data/coolify/services/abc-123-XYZ"
        );
        assert!(ruta_service_dir_segura("").is_err());
        assert!(ruta_service_dir_segura("../otro").is_err());
        assert!(ruta_service_dir_segura("a/b").is_err());
        assert!(ruta_service_dir_segura("x y").is_err());
        assert!(ruta_service_dir_segura("x;y").is_err());
        assert!(ruta_service_dir_segura("x'y").is_err());
    }

    /* [B4-3] La unidad autoheal sigue la misma regla de saneado que el instalador. */
    #[test]
    fn unidad_autoheal_misma_regla_que_instalador() {
        use super::nombre_unidad_autoheal;
        assert_eq!(nombre_unidad_autoheal("cm-test-b4"), "cm-autoheal-cm-test-b4");
        assert_eq!(nombre_unidad_autoheal("mi sitio.raro"), "cm-autoheal-mi-sitio-raro");
    }

    /* [B4-3] El comando de restos cubre timer+imagen+uploads y nunca falla. */
    #[test]
    fn comando_restos_cubre_timer_imagen_uploads() {
        use super::{comando_limpieza_restos_host, nombre_unidad_autoheal};
        let unit = nombre_unidad_autoheal("cm-test-b4");
        let cmd = comando_limpieza_restos_host(&unit, "jgkks048ocwwsogc4k48kk00", "cm-test-b4");
        // La unidad va entrecomillada para shell ('unit'.timer concatena válido en sh).
        assert!(cmd.contains("'cm-autoheal-cm-test-b4'.timer"));
        assert!(cmd.contains("jgkks048ocwwsogc4k48kk00-app"));
        assert!(cmd.contains("/data/uploads/cm-test-b4"));
        assert!(cmd.contains("rmdir"));
        assert!(cmd.contains("|| true"));
        assert!(cmd.contains("DELETE_SITE_RESTOS_OK"));
    }
}
