/* Split 119A-3 de theme_manager.rs — install_glory_theme (instalacion inicial). */

use crate::config::GloryConfig;
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::ssh_client::SshClient;

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

    /* Paso 2+3: clonar tema y libreria Glory. */
    clonar_tema_y_libreria(
        ssh,
        container_id,
        glory_config,
        glory_branch,
        library_branch,
        &theme_dir,
        &glory_dir,
    )
    .await?;

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
    ejecutar_glory_sync(ssh, container_id, &theme_dir).await?;

    tracing::info!("Tema Glory instalado exitosamente en {theme_dir}");
    Ok(())
}

/* Paso 8: Glory sync iterado (resuelve dependencias circulares pagina/opcion). */
async fn ejecutar_glory_sync(
    ssh: &SshClient,
    container_id: &str,
    theme_dir: &str,
) -> std::result::Result<(), CoolifyError> {
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
    tracing::info!(
        "Glory sync: {}",
        result.stdout.lines().last().unwrap_or("sin output")
    );
    Ok(())
}

/* Pasos 2+3: clona el repo del tema y la libreria Glory (submodule). */
async fn clonar_tema_y_libreria(
    ssh: &SshClient,
    container_id: &str,
    glory_config: &GloryConfig,
    glory_branch: &str,
    library_branch: &str,
    theme_dir: &str,
    glory_dir: &str,
) -> std::result::Result<(), CoolifyError> {
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
    Ok(())
}
