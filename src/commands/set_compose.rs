/*
 * set-compose — Fija el docker-compose de un stack existente sin pasar por
 * `deploy-service`.
 *
 * [299A-2] `new --template rust` + `deploy-service` solo sirven al molde glory
 * (app+postgres+theme/Dockerfile.rust). Los stacks no-glory (p. ej. glory-pulse:
 * pull GHCR por tag fijo + proxy de socket) necesitan un compose arbitrario.
 * Garantías por diseño:
 *   1. Resolución SOLO por nombre en settings.json: jamás se acepta un uuid
 *      crudo, así que un typo no puede apuntar a otro stack.
 *   2. Validación fail-closed ANTES de tocar la API: no vacío, tamaño acotado,
 *      ASCII puro (268A-5: Coolify devuelve 422 engañoso ante bytes >127),
 *      clave `services:` de nivel 0 y SIN secciones `build:`/`dockerfile:`
 *      (compilar en la VPS productiva reinició dockerd el 2026-09-20).
 *   3. `--dry-run` muestra lo que se haría sin tocar host ni API.
 *   4. Verificación post-escritura: `get_service` debe reflejar la imagen
 *      fijada; si no, se falla en vez de seguir a ciegas.
 *
 * [por qué no hay parser YAML] El crate no depende de serde_yaml; la validación
 * es heurística por líneas (documentada como tal). La prueba real es el deploy
 * posterior (`start-service` + `health`).
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;

use std::io::Read as _;
use std::path::{Path, PathBuf};

/* Compose mayor de 256 KiB es casi seguro un fichero equivocado. */
const MAX_COMPOSE_BYTES: usize = 256 * 1024;

/// Origen del compose: fichero o stdin (heredoc del operador).
pub enum ComposeSource {
    File(PathBuf),
    Stdin,
}

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    source: &ComposeSource,
    dry_run: bool,
) -> std::result::Result<(), CoolifyError> {
    let compose = leer_fuente(source)?;
    validar_compose(&compose)?;
    let marcador = extraer_marcador_imagen(&compose).ok_or_else(|| {
        CoolifyError::Validation("Compose sin 'image:' a nivel de servicio: no verificable".into())
    })?;

    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    let stack_uuid = site.stack_uuid.clone().ok_or_else(|| {
        CoolifyError::Validation(format!("Sitio '{site_name}' sin stackUuid configurado"))
    })?;
    let dominio = site.dominio.clone();
    let target = settings.resolve_site_target(site)?;

    println!("Fijar compose del stack '{site_name}':");
    println!("  uuid:    {stack_uuid}");
    println!("  dominio: {dominio}");
    println!("  bytes:   {}", compose.len());
    println!("  imagen:  {marcador}");

    if dry_run {
        println!("[dry-run] Se haría, en orden:");
        println!("  1. PATCH /api/v1/services/{stack_uuid} con el compose validado");
        println!("  2. Verificar con GET que el servicio refleja '{marcador}'");
        return Ok(());
    }

    let api = CoolifyApiClient::new(&target.coolify)?;
    api.update_stack_compose(&stack_uuid, &compose).await?;
    verificar_compose_fijado(&api, &stack_uuid, &marcador).await?;
    println!("Compose fijado y verificado en '{site_name}'.");
    Ok(())
}

/* Lee el compose desde fichero o stdin. */
fn leer_fuente(source: &ComposeSource) -> std::result::Result<String, CoolifyError> {
    match source {
        ComposeSource::File(path) => std::fs::read_to_string(path).map_err(|e| {
            CoolifyError::Validation(format!("No se pudo leer {}: {e}", path.display()))
        }),
        ComposeSource::Stdin => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            Ok(buf)
        }
    }
}

/* Validación heurística por líneas (ver nota del módulo). */
fn validar_compose(compose: &str) -> std::result::Result<(), CoolifyError> {
    if compose.trim().is_empty() {
        return Err(CoolifyError::Validation("Compose vacío".into()));
    }
    if compose.len() > MAX_COMPOSE_BYTES {
        return Err(CoolifyError::Validation(format!(
            "Compose de {} bytes supera el máximo de {MAX_COMPOSE_BYTES}",
            compose.len()
        )));
    }
    if !compose.is_ascii() {
        return Err(CoolifyError::Validation(
            "Compose con bytes >127: Coolify lo rechaza con 422 engañoso (268A-5)".into(),
        ));
    }
    if !tiene_clave_services(compose) {
        return Err(CoolifyError::Validation(
            "Compose sin clave 'services:' de nivel 0".into(),
        ));
    }
    if let Some(clave) = clave_build(compose) {
        return Err(CoolifyError::Validation(format!(
            "Compose con '{clave}': el build debe ocurrir fuera de la VPS (imagen por tag fijo)"
        )));
    }
    Ok(())
}

/* `services:` con indentación 0 (ignora comentarios de línea completa). */
fn tiene_clave_services(compose: &str) -> bool {
    compose.lines().any(|linea| {
        if linea.starts_with([' ', '\t']) {
            return false;
        }
        let recortada = linea.trim_end();
        if !recortada.starts_with("services:") {
            return false;
        }
        matches!(
            recortada["services:".len()..].chars().next(),
            None | Some(' ' | '\t' | '#')
        )
    })
}

/* Detecta `build:`/`dockerfile:`/`dockerfile_inline:` anidados bajo un servicio
 * (indentación >2: `services:`=0, servicio=2, clave=4). Heurística documentada. */
fn clave_build(compose: &str) -> Option<&'static str> {
    for linea in compose.lines() {
        let sin_comentario = linea.split('#').next().unwrap_or("").trim_end();
        if sin_comentario.trim_start().is_empty() {
            continue;
        }
        let indent = sin_comentario.len() - sin_comentario.trim_start().len();
        if indent <= 2 {
            continue;
        }
        let clave = sin_comentario.trim_start();
        for candidata in ["build:", "dockerfile:", "dockerfile_inline:"] {
            if clave == candidata
                || clave.starts_with(candidata) && clave[candidata.len()..].starts_with([' ', '\t'])
            {
                return Some(candidata);
            }
        }
    }
    None
}

/* Primera `image:` a nivel de servicio: marcador para la verificación post. */
fn extraer_marcador_imagen(compose: &str) -> Option<String> {
    for linea in compose.lines() {
        let sin_comentario = linea.split('#').next().unwrap_or("").trim_end();
        let indent = sin_comentario.len() - sin_comentario.trim_start().len();
        if indent < 2 {
            continue;
        }
        let clave = sin_comentario.trim_start();
        if let Some(valor) = clave.strip_prefix("image:") {
            let valor = valor.trim().trim_matches('"').to_string();
            if !valor.is_empty() {
                return Some(valor);
            }
        }
    }
    None
}

/* El GET posterior debe reflejar la imagen fijada; si no, fallo fail-closed. */
async fn verificar_compose_fijado(
    api: &CoolifyApiClient,
    stack_uuid: &str,
    marcador: &str,
) -> std::result::Result<(), CoolifyError> {
    let servicio = api.get_service(stack_uuid).await?;
    let plano = serde_json::to_string(&servicio).unwrap_or_default();
    if plano.contains(marcador) {
        Ok(())
    } else {
        Err(CoolifyError::Validation(format!(
            "PATCH aceptado pero GET no refleja '{marcador}': compose no verificado"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "services:\n  pulse:\n    image: ghcr.io/1ndoryu/glory-pulse:sha-804f306\n";

    #[test]
    fn compose_valido_pasa() {
        assert!(validar_compose(BASE).is_ok());
    }

    #[test]
    fn rechaza_vacio_y_sin_services() {
        assert!(validar_compose("").is_err());
        assert!(validar_compose("version: \"3\"\n").is_err());
    }

    #[test]
    fn rechaza_no_ascii() {
        let compose = format!("{BASE}  # acento: á\n");
        assert!(validar_compose(&compose).is_err());
    }

    #[test]
    fn rechaza_build_anidado() {
        for clave in ["build:", "dockerfile: Dockerfile", "dockerfile_inline: |"] {
            let compose = format!("services:\n  app:\n    image: x/y:1\n    {clave}\n");
            assert!(
                validar_compose(&compose).is_err(),
                "debería rechazar {clave}"
            );
        }
    }

    #[test]
    fn acepta_servicio_llamado_build() {
        let compose = "services:\n  build:\n    image: x/y:1\n";
        assert!(validar_compose(compose).is_ok());
    }

    #[test]
    fn marcador_extrae_primera_imagen() {
        assert_eq!(
            extraer_marcador_imagen(BASE),
            Some("ghcr.io/1ndoryu/glory-pulse:sha-804f306".to_string())
        );
        assert_eq!(extraer_marcador_imagen("services:\n"), None);
    }
}
