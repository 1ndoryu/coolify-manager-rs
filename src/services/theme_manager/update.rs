/// Actualiza el tema Glory existente (git pull + rebuild).
/* Split 119A-3 de theme_manager.rs — update_glory_theme (orquestador fino) + ensures/deploy. */

use super::fases::{fase_aplicar_permisos, fase_asegurar_env, fase_compilar_react, fase_ejecutar_migraciones, fase_escribir_php_ini, fase_pull_libreria, fase_pull_tema, fase_repos_sanos, fase_sincronizar_composer, fase_tema_existe};
use super::{base64_encode, obtener_hash_archivo_remoto, CtxActualizacionTema};
use crate::config::GloryConfig;
use crate::domain::{PhpConfig, SmtpConfig};
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;
use crate::services::theme_manager::install_glory_theme;

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

    /* [119A-3] Orquestador fino: cada fase vive en `fase_*` abajo. */
    let ctx = CtxActualizacionTema::nuevo(
        ssh,
        container_id,
        stack_uuid,
        glory_config,
        glory_branch,
        library_branch,
        theme_name,
        skip_react,
        force,
        php_config,
        smtp_config,
        disable_wp_cron,
    );

    /* Verificar que el tema existe */
    if !fase_tema_existe(&ctx).await? {
        tracing::warn!(
            "Tema no encontrado en {}, ejecutando instalacion completa",
            ctx.theme_dir
        );
        return install_glory_theme(
            ssh,
            container_id,
            ctx.glory_config,
            glory_branch,
            library_branch,
            theme_name,
            skip_react,
        )
        .await;
    }

    /* Git safe.directory + auto-heal: verificar que los repos git son validos */
    if !fase_repos_sanos(&ctx).await? {
        tracing::warn!("Repo git del tema roto, re-clonando desde cero");
        let _ = docker::docker_exec(ssh, container_id, &format!("rm -rf {}", ctx.theme_dir)).await;
        return install_glory_theme(
            ssh,
            container_id,
            ctx.glory_config,
            glory_branch,
            library_branch,
            theme_name,
            skip_react,
        )
        .await;
    }

    let composer_hash_antes =
        obtener_hash_archivo_remoto(ssh, container_id, &ctx.composer_lock).await;
    let npm_hash_antes = obtener_hash_archivo_remoto(ssh, container_id, &ctx.npm_lock).await;

    /* Pull del tema + pull de la libreria */
    fase_pull_tema(&ctx).await?;
    fase_pull_libreria(&ctx).await?;

    let composer_hash_despues =
        obtener_hash_archivo_remoto(ssh, container_id, &ctx.composer_lock).await;
    let npm_hash_despues = obtener_hash_archivo_remoto(ssh, container_id, &ctx.npm_lock).await;

    /* [F3/F4] Composer install — verificar que vendor/ existe antes de saltar. */
    fase_sincronizar_composer(&ctx, composer_hash_antes, composer_hash_despues).await?;

    /* .env de produccion */
    fase_asegurar_env(&ctx).await?;

    /* npm build */
    if !ctx.skip_react {
        fase_compilar_react(&ctx, npm_hash_antes, npm_hash_despues).await?;
    }

    /* Ejecutar migraciones pendientes de la BD */
    fase_ejecutar_migraciones(&ctx).await;

    /* Permisos — tema + uploads (uploads puede quedar root:root al recrear contenedor) */
    fase_aplicar_permisos(&ctx).await;

    /* Escribir php.ini con config por tema — sobrevive recreaciones del contenedor */
    fase_escribir_php_ini(&ctx).await;

    /* Desplegar mu-plugin SMTP si hay configuracion SMTP */
    if let Some(smtp) = ctx.smtp_config {
        deploy_smtp_mu_plugin(ssh, container_id, smtp).await;
    }

    /* DISABLE_WP_CRON: desactiva el pseudo-cron de WordPress (basado en visitas HTTP).
     * Los sitios con esta opcion usan system cron del host en su lugar. */
    if ctx.disable_wp_cron {
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
