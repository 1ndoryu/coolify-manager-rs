/* Paso 4 + 4.5: espera por template y reconciliación mariadb [06AA-4 split new_site]. */

use crate::domain::StackTemplate;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

/* Paso 4 + 4.5: espera por template y reconciliación de auth de
 * mariadb antes del tema.
 * [309A-1/F4] Kamples levanta 3 servicios con init pesado (mariadb 10.11
 * + postgres/pgvector con init.sh + wordpress con depends_on): 30 s no
 * llegan (el WP seguía en Created y el flujo abortaba). Espera por
 * template, sin sondeo: el arranque real lo confirma el Paso 5.
 * [01AA-1] En modo imagen, antes asegurar la app levantada: Coolify
 * puede dejar wordpress en Created sin arrancarlo (mariadb/postgres Up,
 * wordpress Created 10+ min hasta `docker start` manual). */
pub(super) async fn esperar_y_reconciliar_db(
    stack_template: &StackTemplate,
    target: &crate::config::DeploymentTargetConfig,
    stack_uuid: &str,
    image: Option<&str>,
) -> std::result::Result<(), CoolifyError> {
    tracing::info!("Esperando a que el stack este listo...");
    let espera_secs = if matches!(stack_template, StackTemplate::Kamples) {
        150
    } else {
        30
    };
    tokio::time::sleep(std::time::Duration::from_secs(espera_secs)).await;

    let es_wordpress = matches!(
        stack_template,
        StackTemplate::Wordpress | StackTemplate::Kamples
    );
    if es_wordpress {
        if image.is_some() {
            asegurar_app_levantada(target, stack_uuid).await?;
        }
        reconciliar_db_auth_mariadb(target, stack_uuid).await?;
    }
    Ok(())
}

/* [01AA-1] Paso 4.5: el UUID de Coolify solo viaja en el path del compose
 * en el host; rechaza cualquier otro valor antes de interpolarlo. */
fn uuid_servicio_valido(uuid: &str) -> bool {
    !uuid.is_empty() && uuid.len() <= 64 && uuid.chars().all(|c| c.is_ascii_alphanumeric())
}

/* [01AA-1] Construye el script de reconciliación (puro, testeable). Todo
 * ocurre en el host del VPS: los secretos se extraen del compose en disco
 * y nunca viajan a la laptop. */
fn script_reconcile_mariadb(stack_uuid: &str) -> std::result::Result<String, CoolifyError> {
    if !uuid_servicio_valido(stack_uuid) {
        return Err(CoolifyError::Validation(format!(
            "UUID de stack inválido para reconciliación: '{stack_uuid}'"
        )));
    }
    Ok(format!(
        concat!(
            "YML=\"/data/coolify/services/{uuid}/docker-compose.yml\"; ",
            "ROOT=$(grep 'MYSQL_ROOT_PASSWORD:' \"$YML\" | head -n 1 | sed 's/.*:[[:space:]]*//;s/[\"'\\''[:space:]]//g'); ",
            "WPP=$(grep 'WORDPRESS_DB_PASSWORD:' \"$YML\" | head -n 1 | sed 's/.*:[[:space:]]*//;s/[\"'\\''[:space:]]//g'); ",
            "case \"$ROOT\" in ''|*[!A-Za-z0-9]*) echo RECONCILE_REFUSE_ROOT; exit 3;; esac; ",
            "case \"$WPP\" in ''|*[!A-Za-z0-9]*) echo RECONCILE_REFUSE_WP; exit 3;; esac; ",
            "docker exec \"mariadb-{uuid}\" mariadb -uroot -p\"$ROOT\" ",
            "-e \"ALTER USER 'manager'@'%' IDENTIFIED BY '$WPP'; FLUSH PRIVILEGES;\" && ",
            "docker exec \"mariadb-{uuid}\" mariadb -umanager -p\"$WPP\" -e \"SELECT 1;\" && ",
            "echo RECONCILE_MARIADB_OK"
        ),
        uuid = stack_uuid
    ))
}

/* [01AA-1] Paso 4.5b: arranca wordpress-<uuid> si Coolify lo dejó en
 * Created/Exited en stacks de imagen. Idempotente (running = no-op).
 * Error duro si tras el arranque no queda en marcha: tema y checks
 * posteriores exigen la app viva. */
async fn asegurar_app_levantada(
    target: &crate::config::DeploymentTargetConfig,
    stack_uuid: &str,
) -> std::result::Result<(), CoolifyError> {
    if !uuid_servicio_valido(stack_uuid) {
        return Err(CoolifyError::Validation(format!(
            "UUID de stack inválido para arranque: {stack_uuid}"
        )));
    }
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;
    let cont = format!("wordpress-{stack_uuid}");
    let estado = ssh
        .execute(&format!(
            "docker inspect {cont} --format '{{{{.State.Status}}}}' 2>&1"
        ))
        .await?;
    if necesita_arranque(estado.stdout.trim()) {
        let arr = ssh.execute(&format!("docker start {cont} 2>&1")).await?;
        if !arr.success() {
            return Err(CoolifyError::Docker {
                exit_code: arr.exit_code,
                stderr: format!("Arranque de {cont} fallido: {}", arr.stderr.trim()),
            });
        }
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        let re = ssh
            .execute(&format!(
                "docker inspect {cont} --format '{{{{.State.Status}}}}' 2>&1"
            ))
            .await?;
        if re.stdout.trim() != "running" {
            return Err(CoolifyError::Validation(format!(
                "'{cont}' no queda en marcha tras docker start (estado: {})",
                re.stdout.trim()
            )));
        }
    }
    tracing::info!("App {cont} en marcha");
    Ok(())
}

/* Estados Docker que exigen arranque: todo lo que no sea running. */
fn necesita_arranque(estado: &str) -> bool {
    estado != "running"
}

/* [01AA-1] Paso 4.5: reconcilia `manager` en mariadb con el
 * WORDPRESS_DB_PASSWORD efectivo. Coolify reescribe MYSQL_PASSWORD tras el
 * create (verificado F4 309A-1 con un único {{DB_PASSWORD}} renderizado),
 * dejando a WP sin acceso a su BD. Idempotente: fijar el valor previsto
 * cuando ya coincide es un no-op. Error duro si falla (el sitio nacería
 * roto y el fallo debe verse, no ocultarse). */
async fn reconciliar_db_auth_mariadb(
    target: &crate::config::DeploymentTargetConfig,
    stack_uuid: &str,
) -> std::result::Result<(), CoolifyError> {
    let script = script_reconcile_mariadb(stack_uuid)?;
    let mut ssh = SshClient::from_vps(&target.vps);
    ssh.connect().await?;
    let out = ssh.execute(&script).await?;
    let combinado = format!("{}\n{}", out.stdout, out.stderr);
    if !out.success() || !out.stdout.contains("RECONCILE_MARIADB_OK") {
        let cola: String = combinado
            .chars()
            .rev()
            .take(600)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        return Err(CoolifyError::Docker {
            exit_code: out.exit_code,
            stderr: format!("Reconciliación DB-auth mariadb fallida: {cola}"),
        });
    }
    tracing::info!("DB-auth mariadb reconciliado para stack {stack_uuid}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_servicio_valido_acepta_coolify_y_rechaza_paths() {
        assert!(uuid_servicio_valido("qogsck08g0ckkww4sc484ow4"));
        assert!(!uuid_servicio_valido(""));
        assert!(!uuid_servicio_valido("../../etc"));
        assert!(!uuid_servicio_valido("abc; rm -rf /"));
        assert!(!uuid_servicio_valido("con espacios"));
    }

    #[test]
    fn script_reconcile_apunta_al_stack_y_verifica_manager() {
        let uuid = "qogsck08g0ckkww4sc484ow4";
        let s = script_reconcile_mariadb(uuid).expect("script válido");
        assert!(s.contains(&format!("/data/coolify/services/{uuid}/docker-compose.yml")));
        assert!(s.contains(&format!("mariadb-{uuid}")));
        assert!(s.contains("ALTER USER 'manager'@'%'"));
        assert!(s.contains("RECONCILE_MARIADB_OK"));
        assert!(!s.contains("6JS55OyYPIAf6DKedmwE2bXd"));
    }

    #[test]
    fn script_reconcile_rechaza_uuid_malicioso() {
        assert!(script_reconcile_mariadb("../../x").is_err());
    }

    #[test]
    fn necesita_arranque_solo_cuando_no_corre() {
        assert!(necesita_arranque("created"));
        assert!(necesita_arranque("exited"));
        assert!(necesita_arranque("paused"));
        assert!(necesita_arranque(""));
        assert!(!necesita_arranque("running"));
    }
}
