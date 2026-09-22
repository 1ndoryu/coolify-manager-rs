/* Split 119A-3 de theme_manager.rs — 8 fases de update_glory_theme + ctx compartido. */

use super::base64_encode;
use super::CtxActualizacionTema;
use crate::error::CoolifyError;
use crate::infra::docker;

/* [119A-3] Fases de `update_glory_theme`. Cada una recibe solo el contexto. */

/// Verifica que el directorio del tema existe en el contenedor.
pub(super) async fn fase_tema_existe(
    ctx: &CtxActualizacionTema<'_>,
) -> std::result::Result<bool, CoolifyError> {
    let check = docker::docker_exec(
        ctx.ssh,
        ctx.container_id,
        &format!("test -d {} && echo 'ok'", ctx.theme_dir),
    )
    .await?;
    Ok(check.stdout.trim() == "ok")
}

/// Registra safe.directory y verifica que los repos git son validos (auto-heal).
pub(super) async fn fase_repos_sanos(
    ctx: &CtxActualizacionTema<'_>,
) -> std::result::Result<bool, CoolifyError> {
    let (ssh, container_id) = (ctx.ssh, ctx.container_id);
    let _ = docker::docker_exec(
        ssh,
        container_id,
        &format!("git config --global --add safe.directory {}", ctx.theme_dir),
    )
    .await;
    let _ = docker::docker_exec(
        ssh,
        container_id,
        &format!("git config --global --add safe.directory {}", ctx.glory_dir),
    )
    .await;
    let git_check = docker::docker_exec(
        ssh,
        container_id,
        &format!("cd {} && git status --short 2>&1", ctx.theme_dir),
    )
    .await?;
    Ok(git_check.success() && !git_check.stderr.contains("not a git repository"))
}

/// Pull del tema (auto-limpia cambios locales; `force` = reset --hard).
pub(super) async fn fase_pull_tema(
    ctx: &CtxActualizacionTema<'_>,
) -> std::result::Result<(), CoolifyError> {
    /* Auto-limpiar cambios locales rastreados antes del pull para evitar conflictos
     * de merge. Los contenedores no deben tener cambios locales — el estado
     * esperado es siempre el del remoto. */
    let pull_cmd = if ctx.force {
        format!(
            "cd {} && git fetch origin && git reset --hard origin/{}",
            ctx.theme_dir, ctx.glory_branch
        )
    } else {
        format!(
            "cd {} && git checkout -- . && git pull origin {}",
            ctx.theme_dir, ctx.glory_branch
        )
    };
    let result = docker::docker_exec(ctx.ssh, ctx.container_id, &pull_cmd).await?;
    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("Error en git pull del tema: {}", result.stderr),
        });
    }
    Ok(())
}

/// Pull de la libreria Glory (best-effort: solo advierte si falla).
pub(super) async fn fase_pull_libreria(
    ctx: &CtxActualizacionTema<'_>,
) -> std::result::Result<(), CoolifyError> {
    let lib_pull = if ctx.force {
        format!(
            "cd {} && git fetch origin && git reset --hard origin/{}",
            ctx.glory_dir, ctx.library_branch
        )
    } else {
        format!(
            "cd {} && git checkout -- . && git pull origin {}",
            ctx.glory_dir, ctx.library_branch
        )
    };
    let result = docker::docker_exec(ctx.ssh, ctx.container_id, &lib_pull).await?;
    if !result.success() {
        tracing::warn!("Git pull de libreria Glory fallo: {}", result.stderr);
    }
    Ok(())
}

/// Composer install si `composer.lock` cambio o si `vendor/` no existe ([F3/F4]).
pub(super) async fn fase_sincronizar_composer(
    ctx: &CtxActualizacionTema<'_>,
    antes: Option<String>,
    despues: Option<String>,
) -> std::result::Result<(), CoolifyError> {
    /* Sin esta verificacion, si el contenedor fue recreado y vendor/ no existe,
     * se salta composer install porque el hash no cambio, dejando el sitio roto. */
    let vendor_exists = docker::docker_exec(
        ctx.ssh,
        ctx.container_id,
        &format!(
            "test -f {}/vendor/autoload.php && echo ok || echo missing",
            ctx.theme_dir
        ),
    )
    .await
    .map(|r| r.stdout.trim() == "ok")
    .unwrap_or(false);

    if antes != despues || !vendor_exists {
        let reason = if !vendor_exists {
            "vendor/autoload.php no existe"
        } else {
            "composer.lock cambio"
        };
        tracing::info!("{reason}, ejecutando composer install...");
        let result = docker::docker_exec(
            ctx.ssh,
            ctx.container_id,
            &format!(
                "cd {} && composer install --no-dev --optimize-autoloader --no-interaction 2>&1",
                ctx.theme_dir
            ),
        )
        .await?;
        if !result.success() {
            tracing::warn!("Composer install fallo: {}", result.stderr);
        }
    } else {
        tracing::info!("composer.lock sin cambios y vendor/ existe, saltando composer install");
    }
    Ok(())
}

/// Asegura el `.env` de produccion del tema.
pub(super) async fn fase_asegurar_env(
    ctx: &CtxActualizacionTema<'_>,
) -> std::result::Result<(), CoolifyError> {
    let env_check = docker::docker_exec(ctx.ssh, ctx.container_id, &format!(
        "test -f {}/.env && echo 'existe' || printf 'DEV=FALSE\nLOCAL=FALSE\nWP_DEBUG=FALSE\n' > {}/.env && echo 'creado'",
        ctx.theme_dir, ctx.theme_dir
    )).await?;
    tracing::info!("Env update: {}", env_check.stdout.trim());
    Ok(())
}

/// npm install (si cambio el lock o falta `node_modules/`) + `npm run build`.
pub(super) async fn fase_compilar_react(
    ctx: &CtxActualizacionTema<'_>,
    antes: Option<String>,
    despues: Option<String>,
) -> std::result::Result<(), CoolifyError> {
    let (ssh, container_id) = (ctx.ssh, ctx.container_id);
    /* Verificar/instalar node si es necesario (puede faltar tras recrear contenedor) */
    let node_check = docker::docker_exec(
        ssh,
        container_id,
        "command -v node > /dev/null 2>&1 && echo ok || echo missing",
    )
    .await?;
    if node_check.stdout.trim() == "missing" {
        tracing::info!("Node no encontrado, instalando...");
        let install_node = "curl -fsSL https://deb.nodesource.com/setup_20.x | bash - > /dev/null 2>&1 && apt-get install -y -qq nodejs > /dev/null 2>&1";
        let _ = docker::docker_exec(ssh, container_id, install_node).await;
    }

    /* [F3/F4] Sin esta verificacion, si el contenedor fue recreado y node_modules/
     * no existe, npm run build falla con exit 127 (vite no encontrado). */
    let node_modules_exists = docker::docker_exec(
        ssh,
        container_id,
        &format!(
            "test -f {}/node_modules/.package-lock.json && echo ok || echo missing",
            ctx.theme_dir
        ),
    )
    .await
    .map(|r| r.stdout.trim() == "ok")
    .unwrap_or(false);

    if antes != despues || !node_modules_exists {
        let reason = if !node_modules_exists {
            "node_modules/ no existe"
        } else {
            "package-lock.json cambio"
        };
        tracing::info!("{reason}, ejecutando npm install...");
        let result = docker::docker_exec(
            ssh,
            container_id,
            &format!(
                "cd {} && npm install --no-audit --no-fund 2>&1",
                ctx.theme_dir
            ),
        )
        .await?;
        if !result.success() {
            return Err(CoolifyError::Docker {
                exit_code: result.exit_code,
                stderr: format!("Error en npm install del tema: {}", result.stderr),
            });
        }
    } else {
        tracing::info!(
            "package-lock.json sin cambios y node_modules/ existe, saltando npm install"
        );
    }

    tracing::info!("Compilando React ({})...", ctx.theme_name);
    let result = docker::docker_exec(
        ssh,
        container_id,
        &format!("cd {} && npm run build 2>&1", ctx.theme_dir),
    )
    .await?;
    if result.success() {
        tracing::info!("React compilado exitosamente.");
        Ok(())
    } else {
        /* [F9] Mostrar tanto stdout como stderr para diagnosticar build failures.
         * Vite y otros bundlers a veces ponen errores en stdout, no en stderr. */
        let combined = if result.stderr.is_empty() {
            result.stdout.clone()
        } else if result.stdout.is_empty() {
            result.stderr.clone()
        } else {
            format!("STDOUT:\n{}\nSTDERR:\n{}", result.stdout, result.stderr)
        };
        let hint = if result.exit_code == 127 {
            " (exit 127 = comando no encontrado, posiblemente falta node_modules/)"
        } else {
            ""
        };
        Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("Error compilando React{hint}:\n{combined}"),
        })
    }
}

/// Ejecuta migraciones pendientes de la BD (best-effort: nunca falla el deploy).
pub(super) async fn fase_ejecutar_migraciones(ctx: &CtxActualizacionTema<'_>) {
    let (ssh, container_id) = (ctx.ssh, ctx.container_id);
    match docker::find_postgres_container(ssh, ctx.stack_uuid).await {
        Ok(pg_container) => {
            let pg_user_res =
                docker::docker_exec(ssh, container_id, "printenv KAMPLES_PG_USER").await;
            let pg_db_res =
                docker::docker_exec(ssh, container_id, "printenv KAMPLES_PG_DBNAME").await;
            match (pg_user_res, pg_db_res) {
                (Ok(u), Ok(d)) => {
                    let pg_user = u.stdout.trim().to_string();
                    let pg_db = d.stdout.trim().to_string();
                    if pg_user.is_empty() || pg_db.is_empty() {
                        tracing::warn!("KAMPLES_PG_USER o KAMPLES_PG_DBNAME no encontradas en el contenedor WP. Saltando migraciones.");
                    } else if let Err(e) =
                        crate::services::theme_migrations::run_pending_migrations(
                            ssh,
                            container_id,
                            &pg_container,
                            ctx.theme_name,
                            &pg_user,
                            &pg_db,
                        )
                        .await
                    {
                        tracing::warn!("Error ejecutando migraciones: {e}. El deploy continua.");
                    }
                }
                _ => tracing::warn!(
                    "No se pudieron leer credenciales PG del contenedor WP. Saltando migraciones."
                ),
            }
        }
        Err(e) => tracing::warn!(
            "No se encontro contenedor PostgreSQL (stack {}): {e}. Saltando migraciones.",
            ctx.stack_uuid
        ),
    }
}

/// Permisos — tema + uploads (uploads puede quedar root:root al recrear contenedor).
pub(super) async fn fase_aplicar_permisos(ctx: &CtxActualizacionTema<'_>) {
    let (ssh, container_id) = (ctx.ssh, ctx.container_id);
    let _ = docker::docker_exec(
        ssh,
        container_id,
        &format!("chown -R www-data:www-data {}", ctx.theme_dir),
    )
    .await;
    let _ = docker::docker_exec(ssh, container_id,
        "bash -c 'mkdir -p /var/www/html/wp-content/uploads && chown -R www-data:www-data /var/www/html/wp-content/uploads && chmod -R 755 /var/www/html/wp-content/uploads'"
    ).await;
}

/// Escribe php.ini con config por tema — sobrevive recreaciones del contenedor.
pub(super) async fn fase_escribir_php_ini(ctx: &CtxActualizacionTema<'_>) {
    let (ssh, container_id) = (ctx.ssh, ctx.container_id);
    let php = ctx.php_config.cloned().unwrap_or_default();
    let ini_content = format!(
        "upload_max_filesize = {}\npost_max_size = {}\nmemory_limit = {}\n",
        php.upload_max_filesize, php.post_max_size, php.memory_limit
    );
    let ini_b64 = base64_encode(&ini_content);
    let _ = docker::docker_exec(
        ssh,
        container_id,
        &format!("bash -c 'echo {ini_b64} | base64 -d > /usr/local/etc/php/conf.d/99-site.ini'"),
    )
    .await;
    tracing::info!(
        "PHP config aplicado: upload={}, post={}, memory={}",
        php.upload_max_filesize,
        php.post_max_size,
        php.memory_limit
    );
}
