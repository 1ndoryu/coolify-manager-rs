/* [14AA-1/F4] Contexto de build desde copias locales (build_mode=builder + repoLocal).
 * El laptop empaqueta lo que compila la VPS de build en dos bundles git:
 *  - app.bundle: la rama del sitio, tal como está commiteada en repoLocal.
 *    Lo no commiteado no entra; la plantilla falla si el commit no casa con CACHE_BUST.
 *  - framework.bundle: el SHA pinado del framework. Se trae de GitHub a un caché
 *    bare del laptop; la VPS de build nunca habla con GitHub ni recibe credenciales.
 * La plantilla Dockerfile.rust-contexto-local va embebida aquí: no se escribe en el VPS 66. */

use super::base_temporal;
use crate::error::CoolifyError;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(super) const PLANTILLA_CONTEXTO_LOCAL: &str =
    include_str!("../../../config/templates/Dockerfile.rust-contexto-local");
pub(super) const FRAMEWORK_REPO: &str = "https://github.com/1ndoryu/glory-rs-framework.git";
/* SHA pinado del framework. FRAMEWORK_TREE es su árbol: lo que verifica la VPS de build
 * (default de ARG FRAMEWORK_TREE en la plantilla; test abajo). */
pub(super) const FRAMEWORK_SHA: &str = "f2419479555e7c8a737ea102a00a4782f79eae34";
pub(super) const FRAMEWORK_TREE: &str = "61fc0c32a452ad74499218b1c6561554be170407";
/* Nombres dentro del contexto de build: la plantilla los referencia por nombre. */
pub(super) const BUNDLE_APP: &str = "app.bundle";
pub(super) const BUNDLE_FRAMEWORK: &str = "framework.bundle";
const REF_FRAMEWORK: &str = "refs/cm/framework";
/* Un fetch inicial del framework o un bundle grande no deben colgar el deploy. */
const TIMEOUT_GIT_SECS: u64 = 600;

pub(super) struct ContextoLocal {
    pub(super) sha12: String,
    pub(super) app_bundle: PathBuf,
    pub(super) framework_bundle: PathBuf,
}

/* Caché bare del framework en el laptop (bajo la base de temporales). */
pub(super) fn cache_dir() -> PathBuf {
    base_temporal().join("cm-contexto")
}

/* Empaqueta sitio y framework en `dir`. Fail-closed: cualquier fallo git aborta
 * antes de tocar la VPS de build. */
pub(super) async fn preparar(
    repo_local: &Path,
    branch: &str,
    dir: &Path,
    cache: &Path,
) -> std::result::Result<ContextoLocal, CoolifyError> {
    let app_bundle = dir.join(BUNDLE_APP);
    let framework_bundle = dir.join(BUNDLE_FRAMEWORK);
    let sha = empaquetar_app(repo_local, branch, &app_bundle).await?;
    empaquetar_framework(
        FRAMEWORK_REPO,
        FRAMEWORK_SHA,
        FRAMEWORK_TREE,
        cache,
        &framework_bundle,
    )
    .await?;
    Ok(ContextoLocal {
        sha12: sha[..12].to_string(),
        app_bundle,
        framework_bundle,
    })
}

/* Bundle de la rama con su historia completa. Devuelve el SHA completo del commit. */
async fn empaquetar_app(
    repo: &Path,
    branch: &str,
    destino: &Path,
) -> std::result::Result<String, CoolifyError> {
    let rama = format!("refs/heads/{branch}");
    let rev = exito(
        git(
            Some(repo),
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{rama}^{{commit}}"),
            ],
            "rev-parse",
        )
        .await?,
        "rev-parse",
    )
    .map_err(|_| {
        CoolifyError::Validation(format!(
            "No se pudo leer la rama '{branch}' en repoLocal '{}' (¿es un repo git con esa rama?)",
            repo.display()
        ))
    })?;
    if rev.len() < 12 || !rev.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(CoolifyError::Validation(format!(
            "rev-parse devolvió un SHA inválido para '{branch}': '{rev}'"
        )));
    }

    let sucio = exito(
        git(
            Some(repo),
            &["status", "--porcelain", "--untracked-files=no"],
            "status",
        )
        .await?,
        "status",
    )?;
    if !sucio.is_empty() {
        println!(
            "      AVISO: '{}' tiene cambios sin commit; no entran en el build (cuenta solo {branch} @ {}).",
            repo.display(),
            &rev[..12]
        );
    }

    /* git bundle create no sobrescribe de forma fiable: se borra el anterior. */
    let ds = destino.to_string_lossy().into_owned();
    let _ = tokio::fs::remove_file(destino).await;
    exito(
        git(
            Some(repo),
            &["bundle", "create", ds.as_str(), rama.as_str()],
            "bundle create",
        )
        .await?,
        "bundle create",
    )?;
    exito(
        git(
            Some(repo),
            &["bundle", "verify", ds.as_str()],
            "bundle verify",
        )
        .await?,
        "bundle verify",
    )?;
    println!(
        "      Contexto app: {branch} @ {} desde {}",
        &rev[..12],
        repo.display()
    );
    Ok(rev)
}

/* Bundle del framework pinado. Solo lleva el ÁRBOL del SHA, en un commit sin padres:
 * el historial del framework arrastra artefactos antiguos (rlib bajo target, ~176 MB) que el
 * build no necesita, y un bundle con historia no se puede traer shallow en la VPS de build.
 * El caché bare es superficial (--depth 1): el árbol completo basta. */
async fn empaquetar_framework(
    origen: &str,
    sha: &str,
    tree_esperado: &str,
    cache: &Path,
    destino: &Path,
) -> std::result::Result<(), CoolifyError> {
    if !cache.join("HEAD").is_file() {
        tokio::fs::create_dir_all(cache).await?;
        let cs = cache.to_string_lossy().into_owned();
        exito(
            git(None, &["init", "--bare", "-q", cs.as_str()], "init").await?,
            "init",
        )?;
    }
    let objeto = format!("{sha}^{{commit}}");
    let presente = git(
        Some(cache),
        &["cat-file", "-e", objeto.as_str()],
        "cat-file",
    )
    .await?
    .status
    .success();
    if !presente {
        exito(
            git(
                Some(cache),
                &["fetch", "--quiet", "--depth", "1", origen, sha],
                "fetch framework",
            )
            .await?,
            "fetch framework",
        )?;
    }
    /* Fail-closed: si el pin ya no apunta a ese árbol, el build no arranca. */
    let tree = exito(
        git(
            Some(cache),
            &["rev-parse", &format!("{sha}^{{tree}}")],
            "rev-parse tree",
        )
        .await?,
        "rev-parse tree",
    )?;
    if tree != tree_esperado {
        return Err(CoolifyError::Validation(format!(
            "El árbol de {sha} es {tree}, no {tree_esperado}: FRAMEWORK_SHA y FRAMEWORK_TREE no casan"
        )));
    }
    /* Identidad fija: el commit es solo un contenedor del árbol, no hace falta que sea reproducible. */
    let huerfano = exito(
        git(
            Some(cache),
            &[
                "-c",
                "user.name=cm",
                "-c",
                "user.email=cm@localhost",
                "commit-tree",
                tree.as_str(),
                "-m",
                &format!("framework {}", &sha[..12]),
            ],
            "commit-tree",
        )
        .await?,
        "commit-tree",
    )?;
    exito(
        git(
            Some(cache),
            &["update-ref", REF_FRAMEWORK, huerfano.as_str()],
            "update-ref",
        )
        .await?,
        "update-ref",
    )?;

    let ds = destino.to_string_lossy().into_owned();
    let _ = tokio::fs::remove_file(destino).await;
    exito(
        git(
            Some(cache),
            &["bundle", "create", ds.as_str(), REF_FRAMEWORK],
            "bundle create framework",
        )
        .await?,
        "bundle create framework",
    )?;
    exito(
        git(
            Some(cache),
            &["bundle", "verify", ds.as_str()],
            "bundle verify framework",
        )
        .await?,
        "bundle verify framework",
    )?;
    println!(
        "      Contexto framework: {sha} (caché {})",
        cache.display()
    );
    Ok(())
}

/* git sin shell (los argumentos no se interpretan) y con timeout. Devuelve la salida
 * cruda: el llamador decide si el código de salida importa (cat-file -e). */
async fn git(
    cwd: Option<&Path>,
    args: &[&str],
    etiqueta: &str,
) -> std::result::Result<std::process::Output, CoolifyError> {
    let mut cmd = tokio::process::Command::new("git");
    if let Some(dir) = cwd {
        cmd.arg("-C").arg(dir);
    }
    cmd.args(args).kill_on_drop(true);
    tokio::time::timeout(Duration::from_secs(TIMEOUT_GIT_SECS), cmd.output())
        .await
        .map_err(|_| {
            CoolifyError::Validation(format!("git {etiqueta} superó {TIMEOUT_GIT_SECS} s"))
        })?
        .map_err(|e| {
            CoolifyError::Validation(format!(
                "git {etiqueta} no se pudo lanzar (¿git instalado?): {e}"
            ))
        })
}

/* Stdout sin espacios si git terminó bien; si no, stderr completo. */
fn exito(
    salida: std::process::Output,
    etiqueta: &str,
) -> std::result::Result<String, CoolifyError> {
    if !salida.status.success() {
        return Err(CoolifyError::Validation(format!(
            "git {etiqueta} falló:\n{}",
            String::from_utf8_lossy(&salida.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&salida.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /* La plantilla local y el árbol pinado deben coincidir: si no, el build verifica un
     * framework distinto del que empaqueta el laptop. */
    #[test]
    fn framework_tree_coincide_con_plantilla() {
        assert!(PLANTILLA_CONTEXTO_LOCAL.contains(&format!("ARG FRAMEWORK_TREE={FRAMEWORK_TREE}")));
        assert!(PLANTILLA_CONTEXTO_LOCAL
            .contains("test \"$(git rev-parse 'HEAD^{tree}')\" = \"${FRAMEWORK_TREE}\""));
    }

    /* Sin clone ni URL de GitHub dentro del build: el contexto llega solo por bundles. */
    #[test]
    fn plantilla_no_clona_y_usa_bundles() {
        assert!(!PLANTILLA_CONTEXTO_LOCAL.contains("git clone"));
        assert!(!PLANTILLA_CONTEXTO_LOCAL.contains("https://github.com"));
        assert!(PLANTILLA_CONTEXTO_LOCAL.contains(&format!("COPY {BUNDLE_APP} /ctx/{BUNDLE_APP}")));
        assert!(PLANTILLA_CONTEXTO_LOCAL
            .contains(&format!("COPY {BUNDLE_FRAMEWORK} /ctx/{BUNDLE_FRAMEWORK}")));
    }

    /* Fail-closed: el build aborta si el commit empaquetado no es el que nombra el tag. */
    #[test]
    fn plantilla_falla_si_el_commit_no_casa() {
        assert!(PLANTILLA_CONTEXTO_LOCAL
            .contains(r#"test "$(git rev-parse --short=12 HEAD)" = "${CACHE_BUST}""#));
    }

    /* Repo temporal real: el bundle contiene la rama pedida y devuelve su SHA. */
    #[tokio::test]
    async fn empaquetar_app_incluye_la_rama() {
        let raiz = super::base_temporal().join(format!("cm-test-contexto-{}", std::process::id()));
        let repo = raiz.join("repo");
        tokio::fs::create_dir_all(&repo).await.unwrap();
        let rs = repo.to_string_lossy().into_owned();
        exito(
            git(None, &["init", "-q", "-b", "main", rs.as_str()], "init")
                .await
                .unwrap(),
            "init",
        )
        .unwrap();
        exito(
            git(
                Some(&repo),
                &[
                    "-c",
                    "user.name=test",
                    "-c",
                    "user.email=test@example.invalid",
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    "init",
                ],
                "commit",
            )
            .await
            .unwrap(),
            "commit",
        )
        .unwrap();

        let destino = raiz.join(BUNDLE_APP);
        let sha = empaquetar_app(&repo, "main", &destino).await.unwrap();
        assert_eq!(sha.len(), 40);
        assert!(destino.is_file());
        let cabecera = exito(
            git(
                Some(&repo),
                &["bundle", "list-heads", destino.to_string_lossy().as_ref()],
                "list-heads",
            )
            .await
            .unwrap(),
            "list-heads",
        )
        .unwrap();
        assert!(cabecera.contains("refs/heads/main"));

        let _ = tokio::fs::remove_dir_all(&raiz).await;
    }

    /* Caché superficial + bundle de solo árbol (el bug del 2026-10-14: con historia, el
     * bundle de 176 MB no llegaba a la VPS de build). El receptor debe recibir un único
     * commit sin padres cuyo árbol es el pinado; con un árbol distinto, falla cerrado. */
    #[tokio::test]
    async fn framework_cache_superficial_da_bundle_de_arbol() {
        let raiz = super::base_temporal().join(format!("cm-test-framework-{}", std::process::id()));
        let origen = raiz.join("origen");
        tokio::fs::create_dir_all(&origen).await.unwrap();
        let os = origen.to_string_lossy().into_owned();
        exito(
            git(None, &["init", "-q", "-b", "main", os.as_str()], "init")
                .await
                .unwrap(),
            "init",
        )
        .unwrap();
        for msg in ["uno", "dos"] {
            exito(
                git(
                    Some(&origen),
                    &[
                        "-c",
                        "user.name=test",
                        "-c",
                        "user.email=test@example.invalid",
                        "commit",
                        "-q",
                        "--allow-empty",
                        "-m",
                        msg,
                    ],
                    "commit",
                )
                .await
                .unwrap(),
                "commit",
            )
            .unwrap();
        }
        let sha = exito(
            git(Some(&origen), &["rev-parse", "HEAD"], "rev-parse")
                .await
                .unwrap(),
            "rev-parse",
        )
        .unwrap();
        let tree = exito(
            git(
                Some(&origen),
                &["rev-parse", "HEAD^{tree}"],
                "rev-parse tree",
            )
            .await
            .unwrap(),
            "rev-parse tree",
        )
        .unwrap();

        /* Caché superficial: tiene el SHA final, pero no su padre. */
        let url = format!("file:///{}", os.replace('\\', "/"));
        let cache = raiz.join("cache");
        let cs = cache.to_string_lossy().into_owned();
        exito(
            git(
                None,
                &[
                    "clone",
                    "-q",
                    "--bare",
                    "--depth",
                    "1",
                    url.as_str(),
                    cs.as_str(),
                ],
                "clone",
            )
            .await
            .unwrap(),
            "clone",
        )
        .unwrap();
        assert!(cache.join("shallow").is_file());

        /* Árbol distinto al pinado: debe fallar antes de escribir el bundle. */
        let malo = raiz.join("malo.bundle");
        let err = empaquetar_framework(&url, &sha, "0".repeat(40).as_str(), &cache, &malo).await;
        assert!(err.is_err());
        assert!(!malo.exists());

        let destino = raiz.join(BUNDLE_FRAMEWORK);
        empaquetar_framework(&url, &sha, &tree, &cache, &destino)
            .await
            .unwrap();

        /* Un repo limpio que recibe el bundle: un único commit sin padres, con el árbol pinado. */
        let receptor = raiz.join("receptor");
        let rs = receptor.to_string_lossy().into_owned();
        exito(
            git(None, &["init", "-q", "--bare", rs.as_str()], "init")
                .await
                .unwrap(),
            "init",
        )
        .unwrap();
        exito(
            git(
                Some(&receptor),
                &[
                    "fetch",
                    "-q",
                    destino.to_string_lossy().as_ref(),
                    "refs/cm/framework:refs/cm/framework",
                ],
                "fetch bundle",
            )
            .await
            .unwrap(),
            "fetch bundle",
        )
        .unwrap();
        let conteo = exito(
            git(
                Some(&receptor),
                &["rev-list", "--count", REF_FRAMEWORK],
                "rev-list",
            )
            .await
            .unwrap(),
            "rev-list",
        )
        .unwrap();
        assert_eq!(conteo, "1");
        let arbol = exito(
            git(
                Some(&receptor),
                &["rev-parse", "refs/cm/framework^{tree}"],
                "rev-parse tree",
            )
            .await
            .unwrap(),
            "rev-parse tree",
        )
        .unwrap();
        assert_eq!(arbol, tree);

        let _ = tokio::fs::remove_dir_all(&raiz).await;
    }
}
