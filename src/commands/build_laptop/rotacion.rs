/* Rotación de tags cm-local (laptop + VPS) [06AA-4 split build_laptop]. */

use crate::infra::ssh_client::SshClient;

/* [01AA-3/F2c] Poda solo imágenes colgadas (sin tag): nunca toca tags
 * válidos (la imagen anterior sigue intacta para rollback). Best-effort:
 * si falla se avisa y se sigue; la poda total manual queda de red. */
pub(crate) async fn podar_colgadas(docker_bin: &str) {
    let salio = tokio::process::Command::new(docker_bin)
        .args(["image", "prune", "-f"])
        .output()
        .await;
    if !matches!(salio, Ok(o) if o.status.success()) {
        eprintln!("      Aviso: poda de capas colgadas falló (nada grave; sigue el flujo).");
    }
}

/* [01AA-3/F2b] Cuántas cm-local/<sitio> se conservan (actual + anterior). */
pub(crate) const ROTACION_MANTENER: usize = 2;

/* Etiquetas a podar en el host indicado: las cm-local/<sitio> menos la
 * recién construida y las (n-1) más recientes. `listado`: líneas
 * "repo:tag<TAB>created" de `docker images --format`. */
fn etiquetas_a_podar(listado: &str, repo_sitio: &str, mantener: &str, n: usize) -> Vec<String> {
    let prefijo = format!("{repo_sitio}:");
    let mut filas: Vec<(&str, &str)> = listado
        .lines()
        .filter_map(|linea| {
            let (imagen, created) = linea.split_once('\t')?;
            if imagen == mantener || !imagen.starts_with(&prefijo) {
                return None;
            }
            Some((imagen, created))
        })
        .collect();
    filas.sort_by(|a, b| b.1.cmp(a.1));
    filas
        .into_iter()
        .skip(n.saturating_sub(1))
        .map(|(imagen, _)| imagen.to_string())
        .collect()
}

/* [01AA-3/F2b] Rotación lado laptop (best-effort: avisa, no aborta). */
pub(crate) async fn rotar_etiquetas_laptop(docker_bin: &str, site_name: &str, mantener: &str) {
    let repo = format!("cm-local/{}", site_name.to_lowercase());
    let listado = tokio::process::Command::new(docker_bin)
        .args([
            "images",
            "--format",
            "{{.Repository}}:{{.Tag}}\t{{.CreatedAt}}",
        ])
        .output()
        .await;
    let listado = match listado {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("      Aviso: no se pudo listar imágenes locales para rotar.");
            return;
        }
    };
    let podar = etiquetas_a_podar(&listado, &repo, mantener, ROTACION_MANTENER);
    if podar.is_empty() {
        return;
    }
    println!(
        "      Rotación laptop: podando {} imagen(es) vieja(s)...",
        podar.len()
    );
    let salio = tokio::process::Command::new(docker_bin)
        .arg("rmi")
        .args(&podar)
        .output()
        .await;
    if !matches!(salio, Ok(o) if o.status.success()) {
        eprintln!("      Aviso: rotación laptop incompleta (se reintenta en el próximo deploy).");
    }
}

/* [01AA-3/F2b] Rotación lado VPS (best-effort: avisa, no aborta el deploy). */
pub(crate) async fn rotar_etiquetas_vps(ssh: &mut SshClient, site_name: &str, mantener: &str) {
    let repo = format!("cm-local/{}", site_name.to_lowercase());
    let listado = match ssh
        .execute("docker images --format '{{.Repository}}:{{.Tag}}\t{{.CreatedAt}}'")
        .await
    {
        Ok(o) if o.success() => o.stdout,
        _ => {
            eprintln!("      Aviso: no se pudo listar imágenes del VPS para rotar.");
            return;
        }
    };
    let podar = etiquetas_a_podar(&listado, &repo, mantener, ROTACION_MANTENER);
    if podar.is_empty() {
        return;
    }
    println!(
        "      Rotación VPS: podando {} imagen(es) vieja(s)...",
        podar.len()
    );
    let args = podar
        .iter()
        .map(|t| format!("'{}'", t.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ");
    if ssh
        .execute(&format!("docker rmi {args}"))
        .await
        .map(|o| o.success())
        .unwrap_or(false)
    {
    } else {
        eprintln!("      Aviso: rotación VPS incompleta (se reintenta en el próximo deploy).");
    }
}

/* [01AA-3/F2] Tag automático Kamples (puro: testeable sin reloj real). */
pub(super) fn etiqueta_kamples(site_name: &str, marca: &str) -> String {
    format!("cm-local/{}:{marca}", site_name.to_lowercase())
}

#[cfg(test)]
mod pruebas_rotacion {
    use super::*;
    use crate::infra::validation::validate_image_ref;

    /* [01AA-3/F2] Tag Kamples válido para validate_image_ref (nunca latest). */
    #[test]
    fn tag_kamples_pasa_validacion() {
        let tag = etiqueta_kamples("Mi-Sitio", "20261001-123456");
        assert_eq!(tag, "cm-local/mi-sitio:20261001-123456");
        validate_image_ref(&tag).unwrap();
    }

    /* [01AA-3/F2b] Con 4 tags y N=2: se podan las 2 más viejas (la actual
     * excluida por `mantener`, la más reciente conservada por N). */
    #[test]
    fn rotacion_conserva_actual_y_anterior() {
        let listado = "cm-local/sitio:20261001-100000\t2026-10-01 10:00:00 +0200 CEST\n\
             cm-local/sitio:20261001-110000\t2026-10-01 11:00:00 +0200 CEST\n\
             cm-local/sitio:20261001-120000\t2026-10-01 12:00:00 +0200 CEST\n\
             cm-local/sitio:20261001-130000\t2026-10-01 13:00:00 +0200 CEST\n\
             cm-local/otro:20261001-140000\t2026-10-01 14:00:00 +0200 CEST\n\
             wordpress:php8.2-apache\t2026-09-01 00:00:00 +0200 CEST\n";
        let podar = etiquetas_a_podar(
            listado,
            "cm-local/sitio",
            "cm-local/sitio:20261001-130000",
            ROTACION_MANTENER,
        );
        assert_eq!(
            podar,
            vec![
                "cm-local/sitio:20261001-110000".to_string(),
                "cm-local/sitio:20261001-100000".to_string(),
            ]
        );
    }

    /* [01AA-3/F2b] Sin viejas no se poda nada (ni la actual). */
    #[test]
    fn rotacion_sin_viejas_no_toca_nada() {
        let listado = "cm-local/sitio:20261001-130000\t2026-10-01 13:00:00 +0200 CEST\n";
        assert!(etiquetas_a_podar(
            listado,
            "cm-local/sitio",
            "cm-local/sitio:20261001-130000",
            ROTACION_MANTENER,
        )
        .is_empty());
    }
}
