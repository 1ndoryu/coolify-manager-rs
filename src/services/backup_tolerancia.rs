/* [19AA-6] Tolerancia del backup pre-deploy a una app en bucle.
 *
 * Con el contenedor en `Restarting`, `docker ps` no lo lista y la copia se
 * queda sin credenciales de la base: abortar el deploy justo entonces impide
 * desplegar la correccion. Se reintenta una vez tras una espera corta y, si
 * sigue igual, se avisa y el deploy continua sin copia de la base. Cualquier
 * otro fallo (disco, permisos, SSH) sigue abortando. Mismo patron que el de
 * pgvector (`database_manager::es_error_pgvector_faltante`). */

use crate::error::{CoolifyError, SshError};
use std::future::Future;
use std::time::Duration;

/* Espera antes del unico reintento: muchos bucles son de arranque y en 10 s ya hay contenedor. */
pub const ESPERA_REINTENTO: Duration = Duration::from_secs(10);

/* Reconoce el contenedor ausente, parado o reiniciandose. `ContainerNotFound`
 * es seguro aqui porque el deploy ya comprobo que el sitio existe en Coolify;
 * el texto de `docker exec` cubre el caso en que el contenedor se cae a mitad. */
pub fn es_error_contenedor_inestable(error: &CoolifyError) -> bool {
    match error {
        CoolifyError::Ssh(SshError::ContainerNotFound { .. }) => true,
        CoolifyError::Docker { stderr, .. } => {
            let texto = stderr.to_lowercase();
            texto.contains("is restarting") || texto.contains("is not running")
        }
        _ => false,
    }
}

/* Ejecuta `intento`; si falla por contenedor inestable, espera y lo repite una sola vez. */
pub async fn con_reintento_por_bucle<F, Fut, T>(
    espera: Duration,
    mut intento: F,
) -> std::result::Result<T, CoolifyError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = std::result::Result<T, CoolifyError>>,
{
    match intento().await {
        Err(e) if es_error_contenedor_inestable(&e) => {
            tracing::warn!("Backup pre-deploy con contenedor inestable, reintento en {espera:?}: {e}");
            println!("      Contenedor inestable ({e}); reintentando la copia en {} s...", espera.as_secs());
            tokio::time::sleep(espera).await;
            intento().await
        }
        otro => otro,
    }
}

/* Aviso comun cuando el deploy sigue sin copia de la base. `operacion`: "deploy", "redeploy"... */
pub fn avisar_sin_backup_db(operacion: &str, error: &CoolifyError) {
    tracing::warn!("Backup pre-{operacion} omitido por contenedor inestable: {error}");
    println!("ADVERTENCIA [19AA-6]: la app esta en bucle o parada y no se pudo copiar su base de datos.");
    println!("ADVERTENCIA [19AA-6]: el {operacion} CONTINUA SIN backup de la base de datos (sin rollback DB). Backup: omitido (contenedor inestable).");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn docker(stderr: &str) -> CoolifyError {
        CoolifyError::Docker { exit_code: 1, stderr: stderr.to_string() }
    }

    fn no_encontrado() -> CoolifyError {
        SshError::ContainerNotFound { filter: "x".to_string() }.into()
    }

    #[test]
    fn clasifica_contenedor_ausente_y_reiniciando() {
        assert!(es_error_contenedor_inestable(&no_encontrado()));
        assert!(es_error_contenedor_inestable(&docker("Error response: Container abc is restarting, wait until the container is running")));
        assert!(es_error_contenedor_inestable(&docker("Container abc is not running")));
    }

    #[test]
    fn no_tolera_otros_fallos() {
        assert!(!es_error_contenedor_inestable(&docker("No space left on device")));
        assert!(!es_error_contenedor_inestable(&SshError::Disconnected.into()));
        assert!(!es_error_contenedor_inestable(&CoolifyError::Validation("x".to_string())));
    }

    #[tokio::test]
    async fn reintenta_una_vez_y_se_recupera() {
        let llamadas = Cell::new(0);
        let r = con_reintento_por_bucle(Duration::ZERO, || async {
            llamadas.set(llamadas.get() + 1);
            if llamadas.get() == 1 { Err(no_encontrado()) } else { Ok(7) }
        })
        .await;
        assert_eq!(r.unwrap(), 7);
        assert_eq!(llamadas.get(), 2);
    }

    #[tokio::test]
    async fn si_el_reintento_falla_devuelve_el_error_para_que_el_llamador_avise() {
        let llamadas = Cell::new(0);
        let r: std::result::Result<(), _> = con_reintento_por_bucle(Duration::ZERO, || async {
            llamadas.set(llamadas.get() + 1);
            Err(no_encontrado())
        })
        .await;
        assert!(es_error_contenedor_inestable(&r.unwrap_err()));
        assert_eq!(llamadas.get(), 2, "un solo reintento, no un bucle");
    }

    #[tokio::test]
    async fn otro_fallo_no_se_reintenta() {
        let llamadas = Cell::new(0);
        let r: std::result::Result<(), _> = con_reintento_por_bucle(Duration::ZERO, || async {
            llamadas.set(llamadas.get() + 1);
            Err(docker("No space left on device"))
        })
        .await;
        assert!(r.is_err());
        assert_eq!(llamadas.get(), 1);
    }
}
