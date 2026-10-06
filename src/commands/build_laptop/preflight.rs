/* Preflight laptop compartido (sitio + fichero) [06AA-4 split build_laptop]. */

use super::pipeline::run_local_checked;
use crate::error::CoolifyError;

use std::path::Path;

/* Preflight laptop compartido (sitio + fichero). */
pub(super) async fn preflight_local(docker_bin: &str) -> std::result::Result<(), CoolifyError> {
    run_local_checked(docker_bin, &["version"], "docker version").await?;
    run_local_checked(docker_bin, &["system", "df"], "docker system df").await?;
    /* [01AA-3/F0] Falla pronto con mensaje claro si queda poco disco
     * (el 01-10 el build murió a mitad con un EOF críptico). */
    verificar_espacio_minimo(&super::work_dir("preflight")?).await?;
    Ok(())
}

/* [01AA-3/F0] Mínimo libre para compilar en laptop (imagen ~2 GB + tgz). */
const MIN_ESPACIO_LIBRE_BYTES: u64 = 5 * 1024 * 1024 * 1024;

async fn verificar_espacio_minimo(dir: &Path) -> std::result::Result<(), CoolifyError> {
    let libres = espacio_libre_bytes(dir).await?;
    if libres < MIN_ESPACIO_LIBRE_BYTES {
        return Err(CoolifyError::Validation(format!(
            "Poco espacio en disco para compilar: libres {:.1} GB, mínimo {} GB. \
             Libera espacio (borra imágenes viejas `docker image prune` o compacta \
             el disco de Docker) y reintenta.",
            libres as f64 / 1_073_741_824.0,
            MIN_ESPACIO_LIBRE_BYTES / 1_073_741_824
        )));
    }
    Ok(())
}

/* Bytes libres del volumen que contiene `dir`. Windows: fsutil; resto: df.
 * Si no se puede medir, error (fail-closed: mejor no compilar a ciegas). */
async fn espacio_libre_bytes(dir: &Path) -> std::result::Result<u64, CoolifyError> {
    #[cfg(windows)]
    {
        let raiz = dir
            .ancestors()
            .last()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| r"C:\".to_string());
        let salida = run_local_checked(
            "fsutil",
            &["volume", "diskfree", raiz.as_str()],
            "fsutil diskfree",
        )
        .await?;
        parsear_fsutil_diskfree(&salida).ok_or_else(|| {
            CoolifyError::Validation(format!(
                "No se pudo leer espacio libre de '{raiz}':\n{salida}"
            ))
        })
    }
    #[cfg(not(windows))]
    {
        let salida =
            run_local_checked("df", &["-B1", &dir.to_string_lossy()], "df espacio libre").await?;
        parsear_df_bloques(&salida).ok_or_else(|| {
            CoolifyError::Validation(format!("No se pudo leer espacio libre vía df:\n{salida}"))
        })
    }
}

/* "Bytes disponibles : 123456789" (fsutil, con o sin separadores de miles). */
#[cfg(any(windows, test))]
fn parsear_fsutil_diskfree(salida: &str) -> Option<u64> {
    salida.lines().find_map(|linea| {
        let (_, valor) = linea.split_once(':')?;
        if !linea.to_lowercase().contains("disponib") {
            return None;
        }
        valor
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u64>()
            .ok()
    })
}

/* Última columna numérica de la 2ª línea de `df -B1` (= disponibles en bytes). */
#[cfg(any(not(windows), test))]
fn parsear_df_bloques(salida: &str) -> Option<u64> {
    let linea = salida.lines().nth(1)?;
    linea.split_whitespace().nth(3)?.parse::<u64>().ok()
}

#[cfg(test)]
mod pruebas_preflight {
    use super::*;

    /* [01AA-3/F0] fsutil con y sin separadores de miles. */
    #[test]
    fn fsutil_parsea_bytes_disponibles() {
        let con_miles = "Espacio total : 1000204886016\r\nBytes disponibles : 3,660,156,928\r\n";
        assert_eq!(parsear_fsutil_diskfree(con_miles), Some(3_660_156_928));
        let sin_miles = "Bytes disponibles : 3660156928\n";
        assert_eq!(parsear_fsutil_diskfree(sin_miles), Some(3_660_156_928));
        assert_eq!(parsear_fsutil_diskfree("nada útil\n"), None);
    }

    /* [01AA-3/F0] df -B1: 4ª columna de la 2ª línea. */
    #[test]
    fn df_parsea_disponibles_en_bytes() {
        let salida = "Filesystem 1B-blocks Used Available Use% Mounted\n/dev/sda1 309229174784 131234 295123456789 1% /\n";
        assert_eq!(parsear_df_bloques(salida), Some(295_123_456_789));
        assert_eq!(parsear_df_bloques("solo cabecera\n"), None);
    }
}
