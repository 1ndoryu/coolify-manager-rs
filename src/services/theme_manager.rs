/*
 * ThemeManager — instalacion y actualizacion del tema Glory.
 * Equivale a WordPress/ThemeManager.psm1.
 */

use crate::config::GloryConfig;
use crate::domain::{PhpConfig, SmtpConfig};
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;

async fn obtener_hash_archivo_remoto(
    ssh: &SshClient,
    container_id: &str,
    ruta: &str,
) -> Option<String> {
    let comando = format!("if [ -f {ruta} ]; then sha256sum {ruta} | awk '{{print $1}}'; fi");

    match docker::docker_exec(ssh, container_id, &comando).await {
        Ok(resultado) if resultado.success() => {
            let hash = resultado.stdout.trim();
            if hash.is_empty() {
                None
            } else {
                Some(hash.to_string())
            }
        }
        _ => None,
    }
}

/// Instala el tema Glory completo dentro del contenedor WordPress.
pub async fn install_glory_theme(
    ssh: &SshClient,
    container_id: &str,
    glory_config: &GloryConfig,
    glory_branch: &str,
    library_branch: &str,
    theme_name: &str,
    skip_react: bool,
) -> std::result::Result<(), CoolifyError> {
    tracing::info!("Instalando tema Glory (branch: {glory_branch}) en contenedor {container_id}");

    let theme_dir = format!("/var/www/html/wp-content/themes/{theme_name}");
    let glory_dir = format!("{theme_dir}/Glory");

    /* Paso 1: Instalar dependencias del sistema */
    let deps_script = r#"apt-get update -qq && apt-get install -y -qq git curl > /dev/null 2>&1
if ! command -v node &> /dev/null; then
    curl -fsSL https://deb.nodesource.com/setup_20.x | bash - > /dev/null 2>&1
    apt-get install -y -qq nodejs > /dev/null 2>&1
fi
if ! command -v composer &> /dev/null; then
    curl -sS https://getcomposer.org/installer | php -- --install-dir=/usr/local/bin --filename=composer > /dev/null 2>&1
fi
node --version 2>/dev/null || echo 'WARN: node no disponible'
composer --version 2>/dev/null || echo 'WARN: composer no disponible'
echo 'Dependencias instaladas'"#;

    let result = docker::docker_exec(ssh, container_id, deps_script).await?;
    if !result.success() {
        tracing::warn!(
            "Algunas dependencias podrian no haberse instalado: {}",
            result.stderr
        );
    }

    /* Paso 2: Clonar repositorio del tema */
    let clone_script = format!(
        r#"if [ -d "{theme_dir}/.git" ]; then
    echo 'Tema ya existe, saltando clonacion'
else
    rm -rf {theme_dir}
    git clone --branch {glory_branch} --single-branch {template_repo} {theme_dir}
fi
git config --global --add safe.directory {theme_dir}
cd {theme_dir} && git checkout {glory_branch} && git pull origin {glory_branch}
echo 'Repositorio del tema listo'"#,
        theme_dir = theme_dir,
        glory_branch = glory_branch,
        template_repo = glory_config.template_repo
    );

    let result = docker::docker_exec(ssh, container_id, &clone_script).await?;
    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("Error clonando tema: {}", result.stderr),
        });
    }

    /* Paso 3: Clonar libreria Glory (submodule) */
    let lib_script = format!(
        r#"if [ -d "{glory_dir}/.git" ]; then
    echo 'Libreria Glory ya existe'
else
    rm -rf {glory_dir}
    git clone --branch {library_branch} --single-branch {library_repo} {glory_dir}
fi
git config --global --add safe.directory {glory_dir}
cd {glory_dir} && git checkout {library_branch} && git pull origin {library_branch}
echo 'Libreria Glory lista'"#,
        glory_dir = glory_dir,
        library_branch = library_branch,
        library_repo = glory_config.library_repo
    );

    let result = docker::docker_exec(ssh, container_id, &lib_script).await?;
    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("Error clonando libreria Glory: {}", result.stderr),
        });
    }

    /* Paso 4: Composer install */
    let composer_script = format!(
        "cd {theme_dir} && composer install --no-dev --optimize-autoloader --no-interaction 2>&1",
        theme_dir = theme_dir
    );
    let result = docker::docker_exec(ssh, container_id, &composer_script).await?;
    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("Error en composer install: {}", result.stderr),
        });
    }

    /* Paso 5: Crear .env de produccion si no existe */
    let env_script = format!(
        r#"if [ ! -f "{theme_dir}/.env" ]; then
    printf 'DEV=FALSE\nLOCAL=FALSE\nWP_DEBUG=FALSE\n' > {theme_dir}/.env
    echo '.env creado'
else
    echo '.env ya existe'
fi"#,
        theme_dir = theme_dir
    );
    let result = docker::docker_exec(ssh, container_id, &env_script).await?;
    tracing::info!("Env: {}", result.stdout.trim());

    /* Paso 6: npm install + build (si no skip_react)
     * Se ejecuta siempre para que cambios en lockfiles/subpaquetes no dependan
     * de node_modules stale dentro del contenedor. */
    if !skip_react {
        let npm_script = format!(
            "cd {theme_dir} && npm install --no-audit --no-fund 2>&1 && npm run build 2>&1",
            theme_dir = theme_dir
        );
        let result = docker::docker_exec(ssh, container_id, &npm_script).await?;
        if !result.success() {
            return Err(CoolifyError::Docker {
                exit_code: result.exit_code,
                stderr: format!(
                    "Error instalando dependencias o compilando React: {}",
                    result.stderr
                ),
            });
        }
    } else {
        tracing::info!("Saltando build de React (--skip-react)");
    }

    /* Paso 7: Permisos */
    let perms_script = format!(
        "chown -R www-data:www-data {theme_dir}",
        theme_dir = theme_dir
    );
    let _ = docker::docker_exec(ssh, container_id, &perms_script).await;

    /* [N3] Paso 8: Glory sync — sincroniza opciones, paginas y contenido por defecto.
     * El script glory_sync.php inicializa OpcionManager, PageManager y DefaultContentSynchronizer.
     * Necesita ejecutarse varias veces en la primera instalacion para que todas las dependencias
     * circulares se resuelvan (paginas que dependen de opciones que dependen de paginas). */
    let sync_script = format!(
        r#"cd {theme_dir}
if [ -f "scripts/glory_sync.php" ]; then
    for i in 1 2 3; do
        php scripts/glory_sync.php 2>&1 || true
        echo "Glory sync iteracion $i completada"
    done
elif [ -f "Glory/scripts/glory_sync.php" ]; then
    for i in 1 2 3; do
        php Glory/scripts/glory_sync.php 2>&1 || true
        echo "Glory sync iteracion $i completada"
    done
else
    echo "WARN: glory_sync.php no encontrado, saltando"
fi"#,
        theme_dir = theme_dir
    );
    let result = docker::docker_exec(ssh, container_id, &sync_script).await?;
    tracing::info!("Glory sync: {}", result.stdout.lines().last().unwrap_or("sin output"));

    tracing::info!("Tema Glory instalado exitosamente en {theme_dir}");
    Ok(())
}

/// Ejecuta las migraciones SQL pendientes contra el contenedor PostgreSQL del stack.
///
/// Estrategia:
/// - Lee los archivos .sql del directorio de migraciones del tema (orden alfabetico garantiza version correcta).
/// - Usa una tabla `_migraciones_ejecutadas` en PG para tracking idempotente.
/// - Solo ejecuta archivos cuyo nombre todavia no esta registrado.
/// - Los archivos v001_* (schema inicial/base) se saltan — asume que BD ya existe.
pub async fn run_pending_migrations(
    ssh: &SshClient,
    wp_container_id: &str,
    pg_container_id: &str,
    theme_name: &str,
    pg_user: &str,
    pg_db: &str,
) -> std::result::Result<(), CoolifyError> {
    let migrations_dir =
        format!("/var/www/html/wp-content/themes/{theme_name}/App/Kamples/Database/migrations");

    /* Asegurar tabla de tracking en PG */
    let create_tracking = format!(
        "psql -U {pg_user} -d {pg_db} -c \"CREATE TABLE IF NOT EXISTS _migraciones_ejecutadas (nombre VARCHAR(255) PRIMARY KEY, ejecutada_en TIMESTAMPTZ DEFAULT NOW());\"",
    );
    let result = docker::docker_exec(ssh, pg_container_id, &create_tracking).await?;
    if !result.success() {
        tracing::warn!(
            "No se pudo crear tabla de tracking de migraciones: {}",
            result.stderr
        );
        return Ok(());
    }

    /* Listar archivos SQL disponibles ordenados */
    let list_cmd = format!("ls {migrations_dir}/v*.sql 2>/dev/null | sort");
    let list_result = docker::docker_exec(ssh, wp_container_id, &list_cmd).await?;
    if list_result.stdout.trim().is_empty() {
        tracing::info!("No se encontraron archivos .sql en {migrations_dir}");
        return Ok(());
    }

    let sql_files: Vec<&str> = list_result.stdout.trim().lines().collect();
    tracing::info!("Migraciones disponibles: {}", sql_files.len());

    for file_path in sql_files {
        let file_name = file_path.split('/').next_back().unwrap_or(file_path);

        /* Saltar schemas iniciales — la BD ya fue creada en el deploy inicial */
        if file_name.starts_with("v001_") {
            tracing::debug!("Saltando schema inicial: {file_name}");
            continue;
        }

        /* Verificar si ya fue ejecutada */
        let check_cmd = format!(
            "psql -U {pg_user} -d {pg_db} -t -c \"SELECT COUNT(*) FROM _migraciones_ejecutadas WHERE nombre = '{file_name}';\"",
        );
        let check = docker::docker_exec(ssh, pg_container_id, &check_cmd).await?;
        let count: i32 = check.stdout.trim().parse().unwrap_or(0);
        if count > 0 {
            tracing::debug!("Migracion ya ejecutada: {file_name}");
            continue;
        }

        /* Copiar SQL del contenedor WP al contenedor PG via docker cp en el host */
        tracing::info!("Ejecutando migracion: {file_name}");

        /* Extraer SQL del contenedor WordPress y ejecutarlo en PG */
        let exec_cmd = format!(
            "SQL=$(docker exec {wp_container_id} cat {file_path}); echo \"$SQL\" | docker exec -i {pg_container_id} psql -U {pg_user} -d {pg_db} 2>&1",
        );
        let exec_result = ssh.execute(&exec_cmd).await?;

        if exec_result.success() {
            /* Registrar como ejecutada */
            let register_cmd = format!(
                "psql -U {pg_user} -d {pg_db} -c \"INSERT INTO _migraciones_ejecutadas (nombre) VALUES ('{file_name}') ON CONFLICT DO NOTHING;\"",
            );
            let _ = docker::docker_exec(ssh, pg_container_id, &register_cmd).await;
            tracing::info!("Migracion completada: {file_name}");
        } else {
            /* Errores de columna ya existente o constraint duplicado son no-fatales (IF NOT EXISTS) */
            let stderr = &exec_result.stderr;
            if stderr.contains("already exists") || stderr.contains("duplicate") {
                tracing::warn!(
                    "Migracion {file_name} con warnings no fatales (ya existe): {stderr}"
                );
                let register_cmd = format!(
                    "psql -U {pg_user} -d {pg_db} -c \"INSERT INTO _migraciones_ejecutadas (nombre) VALUES ('{file_name}') ON CONFLICT DO NOTHING;\"",
                );
                let _ = docker::docker_exec(ssh, pg_container_id, &register_cmd).await;
            } else {
                tracing::error!("Error en migracion {file_name}: {stderr}");
            }
        }
    }

    tracing::info!("Runner de migraciones completado");
    Ok(())
}

/// Actualiza el tema Glory existente (git pull + rebuild).
#[allow(clippy::too_many_arguments)]
pub async fn update_glory_theme(
    ssh: &SshClient,
    container_id: &str,
    stack_uuid: &str,
    glory_config: &GloryConfig,
    glory_branch: &str,
    library_branch: &str,
    theme_name: &str,
    skip_react: bool,
    force: bool,
    php_config: Option<&PhpConfig>,
    smtp_config: Option<&SmtpConfig>,
    disable_wp_cron: bool,
) -> std::result::Result<(), CoolifyError> {
    tracing::info!("Actualizando tema Glory (branch: {glory_branch}) en contenedor {container_id}");

    let theme_dir = format!("/var/www/html/wp-content/themes/{theme_name}");
    let glory_dir = format!("{theme_dir}/Glory");
    let composer_lock = format!("{theme_dir}/composer.lock");
    let npm_lock = format!("{theme_dir}/package-lock.json");

    /* Verificar que el tema existe */
    let check = docker::docker_exec(
        ssh,
        container_id,
        &format!("test -d {theme_dir} && echo 'ok'"),
    )
    .await?;
    if check.stdout.trim() != "ok" {
        tracing::warn!("Tema no encontrado en {theme_dir}, ejecutando instalacion completa");
        return install_glory_theme(
            ssh,
            container_id,
            glory_config,
            glory_branch,
            library_branch,
            theme_name,
            skip_react,
        )
        .await;
    }

    /* Git safe.directory */
    let _ = docker::docker_exec(
        ssh,
        container_id,
        &format!("git config --global --add safe.directory {theme_dir}"),
    )
    .await;
    let _ = docker::docker_exec(
        ssh,
        container_id,
        &format!("git config --global --add safe.directory {glory_dir}"),
    )
    .await;

    /* Auto-heal: verificar que los repos git son validos */
    let git_check = docker::docker_exec(
        ssh,
        container_id,
        &format!("cd {theme_dir} && git status --short 2>&1"),
    )
    .await?;
    if !git_check.success() || git_check.stderr.contains("not a git repository") {
        tracing::warn!("Repo git del tema roto, re-clonando desde cero");
        let _ = docker::docker_exec(ssh, container_id, &format!("rm -rf {theme_dir}")).await;
        return install_glory_theme(
            ssh,
            container_id,
            glory_config,
            glory_branch,
            library_branch,
            theme_name,
            skip_react,
        )
        .await;
    }

    let composer_hash_antes = obtener_hash_archivo_remoto(ssh, container_id, &composer_lock).await;
    let npm_hash_antes = obtener_hash_archivo_remoto(ssh, container_id, &npm_lock).await;

    /* Pull del tema */
    /* Auto-limpiar cambios locales rastreados antes del pull para evitar conflictos de merge.
     * Los contenedores no deben tener cambios locales — el estado esperado es siempre el del remoto. */
    let pull_cmd = if force {
        format!(
            "cd {theme_dir} && git fetch origin && git reset --hard origin/{glory_branch}",
            theme_dir = theme_dir,
            glory_branch = glory_branch
        )
    } else {
        format!(
            "cd {theme_dir} && git checkout -- . && git pull origin {glory_branch}",
            theme_dir = theme_dir,
            glory_branch = glory_branch
        )
    };
    let result = docker::docker_exec(ssh, container_id, &pull_cmd).await?;
    if !result.success() {
        return Err(CoolifyError::Docker {
            exit_code: result.exit_code,
            stderr: format!("Error en git pull del tema: {}", result.stderr),
        });
    }

    /* Pull de la libreria */
    let lib_pull = if force {
        format!(
            "cd {glory_dir} && git fetch origin && git reset --hard origin/{library_branch}",
            glory_dir = glory_dir,
            library_branch = library_branch
        )
    } else {
        format!(
            "cd {glory_dir} && git checkout -- . && git pull origin {library_branch}",
            glory_dir = glory_dir,
            library_branch = library_branch
        )
    };
    let result = docker::docker_exec(ssh, container_id, &lib_pull).await?;
    if !result.success() {
        tracing::warn!("Git pull de libreria Glory fallo: {}", result.stderr);
    }

    let composer_hash_despues =
        obtener_hash_archivo_remoto(ssh, container_id, &composer_lock).await;
    let npm_hash_despues = obtener_hash_archivo_remoto(ssh, container_id, &npm_lock).await;

    /* [F3/F4] Composer install — verificar que vendor/ existe antes de saltar.
     * Sin esta verificacion, si el contenedor fue recreado y vendor/ no existe,
     * se salta composer install porque el hash no cambio, dejando el sitio roto. */
    let vendor_exists = docker::docker_exec(
        ssh,
        container_id,
        &format!("test -f {theme_dir}/vendor/autoload.php && echo ok || echo missing"),
    )
    .await
    .map(|r| r.stdout.trim() == "ok")
    .unwrap_or(false);

    if composer_hash_antes != composer_hash_despues || !vendor_exists {
        let reason = if !vendor_exists {
            "vendor/autoload.php no existe"
        } else {
            "composer.lock cambio"
        };
        tracing::info!("{reason}, ejecutando composer install...");
        let result = docker::docker_exec(ssh, container_id, &format!("cd {theme_dir} && composer install --no-dev --optimize-autoloader --no-interaction 2>&1")).await?;
        if !result.success() {
            tracing::warn!("Composer install fallo: {}", result.stderr);
        }
    } else {
        tracing::info!("composer.lock sin cambios y vendor/ existe, saltando composer install");
    }

    /* .env de produccion */
    let env_check = docker::docker_exec(ssh, container_id, &format!(
        "test -f {theme_dir}/.env && echo 'existe' || printf 'DEV=FALSE\nLOCAL=FALSE\nWP_DEBUG=FALSE\n' > {theme_dir}/.env && echo 'creado'"
    )).await?;
    tracing::info!("Env update: {}", env_check.stdout.trim());

    /* npm build */
    if !skip_react {
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

        /* [F3/F4] npm install — verificar que node_modules/ existe antes de saltar.
         * Sin esta verificacion, si el contenedor fue recreado y node_modules/ no existe,
         * npm run build falla con exit 127 (vite no encontrado). */
        let node_modules_exists = docker::docker_exec(
            ssh,
            container_id,
            &format!("test -f {theme_dir}/node_modules/.package-lock.json && echo ok || echo missing"),
        )
        .await
        .map(|r| r.stdout.trim() == "ok")
        .unwrap_or(false);

        if npm_hash_antes != npm_hash_despues || !node_modules_exists {
            let reason = if !node_modules_exists {
                "node_modules/ no existe"
            } else {
                "package-lock.json cambio"
            };
            tracing::info!("{reason}, ejecutando npm install...");
            let result = docker::docker_exec(
                ssh,
                container_id,
                &format!("cd {theme_dir} && npm install --no-audit --no-fund 2>&1"),
            )
            .await?;
            if !result.success() {
                return Err(CoolifyError::Docker {
                    exit_code: result.exit_code,
                    stderr: format!("Error en npm install del tema: {}", result.stderr),
                });
            }
        } else {
            tracing::info!("package-lock.json sin cambios y node_modules/ existe, saltando npm install");
        }

        tracing::info!("Compilando React ({theme_name})...");
        let result = docker::docker_exec(
            ssh,
            container_id,
            &format!("cd {theme_dir} && npm run build 2>&1"),
        )
        .await?;
        if result.success() {
            tracing::info!("React compilado exitosamente.");
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
            return Err(CoolifyError::Docker {
                exit_code: result.exit_code,
                stderr: format!("Error compilando React{hint}:\n{combined}"),
            });
        }
    }

    /* Ejecutar migraciones pendientes de la BD */
    match docker::find_postgres_container(ssh, stack_uuid).await {
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
                    } else if let Err(e) = run_pending_migrations(
                        ssh,
                        container_id,
                        &pg_container,
                        theme_name,
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
            "No se encontro contenedor PostgreSQL (stack {stack_uuid}): {e}. Saltando migraciones."
        ),
    }

    /* Permisos — tema + uploads (uploads puede quedar root:root al recrear contenedor) */
    let _ = docker::docker_exec(
        ssh,
        container_id,
        &format!("chown -R www-data:www-data {theme_dir}"),
    )
    .await;
    let _ = docker::docker_exec(ssh, container_id,
        "bash -c 'mkdir -p /var/www/html/wp-content/uploads && chown -R www-data:www-data /var/www/html/wp-content/uploads && chmod -R 755 /var/www/html/wp-content/uploads'"
    ).await;

    /* Escribir php.ini con config por tema — sobrevive recreaciones del contenedor */
    let php = php_config.cloned().unwrap_or_default();
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

    /* Desplegar mu-plugin SMTP si hay configuracion SMTP */
    if let Some(smtp) = smtp_config {
        deploy_smtp_mu_plugin(ssh, container_id, smtp).await;
    }

    /* DISABLE_WP_CRON: desactiva el pseudo-cron de WordPress (basado en visitas HTTP).
     * Los sitios con esta opcion usan system cron del host en su lugar. */
    if disable_wp_cron {
        ensure_wp_cron_disabled(ssh, container_id).await;
    }

    /* Habilitar mod_headers si no esta activo (necesario para cache y CORS) */
    let _ = docker::docker_exec(ssh, container_id, "a2enmod headers 2>/dev/null || true").await;

    /* CORS para archivos de audio: desktop Tauri necesita Access-Control-Allow-Origin
     * en archivos estaticos (mp3, etc.) servidos directamente por Apache. */
    ensure_audio_cors_htaccess(ssh, container_id).await;

    /* Actualizar server.ts del contenedor WebSocket si existe en el stack */
    update_websocket_server(ssh, container_id, stack_uuid, theme_name).await;

    /* Graceful restart: aplica nuevo php.ini a workers nuevos sin matar PID 1 */
    let _ = docker::docker_exec(ssh, container_id, "apachectl graceful 2>/dev/null || true").await;

    /* Limpiar OPcache via HTTP: apachectl graceful NO limpia shared memory de OPcache.
     * La única forma desde dentro del contenedor es ejecutar opcache_reset() en el proceso Apache. */
    let opcache_b64 = "PD9waHAgb3BjYWNoZV9yZXNldCgpOyBlY2hvICJvayI7"; /* <?php opcache_reset(); echo "ok"; */
    let oc_script = "/var/www/html/_oc_deploy.php";
    let _ = docker::docker_exec(ssh, container_id, &format!(
        "bash -c 'echo {opcache_b64} | base64 -d > {oc_script} && curl -s http://localhost/_oc_deploy.php && rm -f {oc_script}'"
    )).await;
    tracing::info!("OPcache limpiado (opcache_reset via HTTP).");

    tracing::info!("Tema Glory actualizado exitosamente");
    Ok(())
}

const CORS_AUDIO_MARKER: &str = "CORS STATIC ASSETS DESKTOP";

/// Inyecta headers CORS para archivos estaticos en .htaccess (WP root).
/// Permite que la app desktop/Android Tauri (localhost, tauri.localhost) haga fetch
/// de audio, imagenes, JSON (waveforms) y otros assets servidos por Apache sin PHP.
/// Usa SetEnvIf con whitelist de origenes — no wildcard.
async fn ensure_audio_cors_htaccess(ssh: &SshClient, container_id: &str) {
    let htaccess = "/var/www/html/.htaccess";

    /* Eliminar bloque anterior si existe (puede tener marker viejo "CORS AUDIO DESKTOP") */
    let cleanup_old = format!(
        "sed -i '/# BEGIN CORS AUDIO DESKTOP/,/# END CORS AUDIO DESKTOP/d' {ht} 2>/dev/null; \
         sed -i '/# BEGIN {marker}/,/# END {marker}/d' {ht} 2>/dev/null || true",
        ht = htaccess,
        marker = CORS_AUDIO_MARKER
    );
    let _ = docker::docker_exec(ssh, container_id, &cleanup_old).await;

    let cors_block = format!(
        r#"
# BEGIN {marker}
<IfModule mod_headers.c>
    <FilesMatch "\.(mp3|ogg|wav|webm|flac|jpg|jpeg|png|gif|webp|svg|json)$">
        SetEnvIf Origin "^https?://localhost(:[0-9]+)?$" CORS_ORIGIN=$0
        SetEnvIf Origin "^tauri://localhost$" CORS_ORIGIN=$0
        SetEnvIf Origin "^https?://tauri\.localhost$" CORS_ORIGIN=$0
        SetEnvIf Origin "^https?://127\.0\.0\.1(:[0-9]+)?$" CORS_ORIGIN=$0
        SetEnvIf Origin "^https?://10\.(0\.2\.2|8\.0\.2)(:[0-9]+)?$" CORS_ORIGIN=$0
        Header set Access-Control-Allow-Origin "%{{CORS_ORIGIN}}e" env=CORS_ORIGIN
        Header set Access-Control-Allow-Methods "GET, HEAD, OPTIONS" env=CORS_ORIGIN
        Header set Access-Control-Allow-Headers "Authorization, Content-Type, X-Kamples-Auth, Cache-Control" env=CORS_ORIGIN
        Header set Access-Control-Allow-Credentials "true" env=CORS_ORIGIN
        Header set Vary "Origin" env=CORS_ORIGIN
    </FilesMatch>
</IfModule>
# END {marker}
"#,
        marker = CORS_AUDIO_MARKER
    );

    let cors_b64 = base64_encode(&cors_block);
    let cmd = format!("bash -c 'echo {cors_b64} | base64 -d >> {htaccess}'");

    if let Ok(r) = docker::docker_exec(ssh, container_id, &cmd).await {
        if r.success() {
            tracing::info!("CORS static assets headers inyectados en .htaccess");
        }
    }
}

/// Actualiza server.ts del contenedor WebSocket con la version del tema recien pulleado.
/// Extrae el archivo del contenedor WP y lo inyecta en el contenedor WS via host.
async fn update_websocket_server(
    ssh: &SshClient,
    wp_container_id: &str,
    stack_uuid: &str,
    theme_name: &str,
) {
    let ws_container = match docker::find_websocket_container(ssh, stack_uuid).await {
        Ok(id) => id,
        Err(_) => {
            tracing::debug!(
                "Contenedor WebSocket no encontrado en stack {stack_uuid} — saltando update WS"
            );
            return;
        }
    };

    let server_ts_path =
        format!("/var/www/html/wp-content/themes/{theme_name}/websocket-server/server.ts");

    /* Extraer server.ts del contenedor WP y copiarlo al WS via host /tmp */
    let copy_cmd = format!(
        "docker cp {wp_container_id}:{server_ts_path} /tmp/_ws_server.ts && \
         docker cp /tmp/_ws_server.ts {ws_container}:/app/server.ts && \
         rm -f /tmp/_ws_server.ts"
    );

    match ssh.execute(&copy_cmd).await {
        Ok(r) if r.success() => {
            tracing::info!("server.ts copiado al contenedor WebSocket {ws_container}");
            /* Restart del contenedor WS para aplicar cambios */
            let restart = format!("docker restart {ws_container}");
            match ssh.execute(&restart).await {
                Ok(r) if r.success() => tracing::info!("Contenedor WebSocket reiniciado"),
                Ok(r) => tracing::warn!("Error reiniciando WS: {}", r.stderr),
                Err(e) => tracing::warn!("Error reiniciando WS: {e}"),
            }
        }
        Ok(r) => tracing::warn!("Error copiando server.ts al WS: {}", r.stderr),
        Err(e) => tracing::warn!("Error copiando server.ts al WS: {e}"),
    }
}

/// Despliega un mu-plugin que configura PHPMailer para usar SMTP externo.
/// El mu-plugin NO usa credenciales hardcodeadas — las lee de env vars en tiempo de ejecucion.
async fn deploy_smtp_mu_plugin(ssh: &SshClient, container_id: &str, smtp: &SmtpConfig) {
    let mu_dir = "/var/www/html/wp-content/mu-plugins";
    let mu_file = format!("{mu_dir}/00-smtp-config.php");

    /* Generar el mu-plugin con los valores del config pero fallback a env vars para secrets */
    let plugin_content = format!(
        r#"<?php
/**
 * MU-Plugin: Configuracion SMTP para wp_mail.
 * Generado automaticamente por coolify-manager en cada deploy.
 * Secrets sensibles se leen de env vars para no almacenarse en disco.
 */
add_action('phpmailer_init', function(PHPMailer\PHPMailer\PHPMailer $mailer): void {{
    $host     = getenv('SMTP_HOST')     ?: '{host}';
    $port     = (int)(getenv('SMTP_PORT')     ?: '{port}');
    $user     = getenv('SMTP_USER')     ?: '{user}';
    $pass     = getenv('SMTP_PASS')     ?: '{password}';
    $secure   = getenv('SMTP_SECURE')   ?: '{secure}';
    $from     = getenv('SMTP_FROM')     ?: '{from_email}';
    $fromName = getenv('SMTP_FROM_NAME') ?: '{from_name}';

    if (empty($host) || empty($user) || empty($pass)) {{
        /* Sin config completa, no sobreescribir — dejamos que falle con error claro */
        return;
    }}

    $mailer->isSMTP();
    $mailer->Host       = $host;
    $mailer->Port       = $port;
    $mailer->SMTPAuth   = true;
    $mailer->Username   = $user;
    $mailer->Password   = $pass;
    $mailer->SMTPSecure = $secure === 'ssl' ? PHPMailer\PHPMailer\PHPMailer::ENCRYPTION_SMTPS
                        : ($secure === 'tls' ? PHPMailer\PHPMailer\PHPMailer::ENCRYPTION_STARTTLS : '');
    $mailer->setFrom($from, $fromName);
}}, 10, 1);

/* Forzar el From correcto en todos los correos */
add_filter('wp_mail_from', fn($email) => getenv('SMTP_FROM') ?: '{from_email}');
add_filter('wp_mail_from_name', fn($name) => getenv('SMTP_FROM_NAME') ?: '{from_name}');
"#,
        host = smtp.host,
        port = smtp.port,
        user = smtp.user,
        password = smtp.password,
        from_email = smtp.from_email,
        from_name = smtp.from_name,
        secure = smtp.secure,
    );

    let content_b64 = base64_encode(&plugin_content);
    let result = docker::docker_exec(ssh, container_id, &format!(
        "bash -c 'mkdir -p {mu_dir} && echo {content_b64} | base64 -d > {mu_file} && chown www-data:www-data {mu_file}'"
    )).await;

    match result {
        Ok(r) if r.success() => tracing::info!("MU-plugin SMTP desplegado en {mu_file}"),
        Ok(r) => tracing::warn!("Error desplegando mu-plugin SMTP: {}", r.stderr),
        Err(e) => tracing::warn!("Error desplegando mu-plugin SMTP: {e}"),
    }
}

/// Asegura que DISABLE_WP_CRON esta definido en wp-config.php.
/// WP pseudo-cron depende de visitas HTTP — sin trafico, las tareas programadas no se ejecutan.
/// Los sitios con esta opcion usan system cron del host (`*/5 * * * * curl localhost/wp-cron.php`).
async fn ensure_wp_cron_disabled(ssh: &SshClient, container_id: &str) {
    let wp_config = "/var/www/html/wp-config.php";

    /* Verificar si ya existe */
    let check = docker::docker_exec(
        ssh,
        container_id,
        &format!("grep -q 'DISABLE_WP_CRON' {wp_config} && echo 'existe' || echo 'falta'"),
    )
    .await;

    match check {
        Ok(r) if r.stdout.trim() == "existe" => {
            tracing::info!("DISABLE_WP_CRON ya configurado en wp-config.php");
        }
        _ => {
            /* Insertar antes de la linea "That's all" o al final del bloque de defines */
            let cmd = format!(
                r#"sed -i "/\/\* That's all/i define( 'DISABLE_WP_CRON', true );" {wp_config} || \
                   sed -i "/table_prefix/a define( 'DISABLE_WP_CRON', true );" {wp_config}"#,
            );
            let result = docker::docker_exec(ssh, container_id, &cmd).await;
            match result {
                Ok(r) if r.success() => {
                    tracing::info!("DISABLE_WP_CRON agregado a wp-config.php");
                }
                Ok(r) => tracing::warn!("Error agregando DISABLE_WP_CRON: {}", r.stderr),
                Err(e) => tracing::warn!("Error agregando DISABLE_WP_CRON: {e}"),
            }
        }
    }
}

/// Codifica un string en base64 (sin dependencia externa — usa solo std).
fn base64_encode(input: &str) -> String {
    use std::fmt::Write;
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        let n = (b0 << 16) | (b1 << 8) | b2;
        let _ = write!(
            out,
            "{}{}{}{}",
            TABLE[(n >> 18) & 0x3F] as char,
            TABLE[(n >> 12) & 0x3F] as char,
            if chunk.len() > 1 {
                TABLE[(n >> 6) & 0x3F] as char
            } else {
                '='
            },
            if chunk.len() > 2 {
                TABLE[n & 0x3F] as char
            } else {
                '='
            },
        );
    }
    out
}
