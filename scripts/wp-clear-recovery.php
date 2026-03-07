<?php
/*
 * Limpia el recovery mode de WordPress y resetea OPcache.
 * Usar cuando un error se corrigio pero WP sigue mostrando "error critico".
 */

error_reporting(E_ALL);
ini_set('display_errors', '1');

define('ABSPATH', '/var/www/html/');
$_SERVER['HTTP_HOST'] = 'localhost';
$_SERVER['REQUEST_URI'] = '/';

require_once('/var/www/html/wp-load.php');

global $wpdb;

echo "=== LIMPIEZA RECOVERY MODE ===\n\n";

/* 1. Eliminar recovery keys */
$deleted = $wpdb->query($wpdb->prepare("DELETE FROM wp_options WHERE option_name = %s", 'recovery_keys'));
echo "[1] recovery_keys eliminadas: $deleted filas\n";

/* 2. Eliminar recovery mode email timestamp */
$deleted = $wpdb->query($wpdb->prepare("DELETE FROM wp_options WHERE option_name = %s", 'recovery_mode_email_last_sent'));
echo "[2] recovery_mode_email_last_sent eliminado: $deleted filas\n";

/* 3. Limpiar paused themes/plugins (por si acaso) */
delete_option('_paused_themes');
delete_option('_paused_plugins');
echo "[3] _paused_themes y _paused_plugins limpiados\n";

/* 4. Limpiar transients de error */
$deleted = $wpdb->query($wpdb->prepare(
    "DELETE FROM wp_options WHERE option_name LIKE %s OR option_name LIKE %s OR option_name LIKE %s",
    '%_transient_%fatal%',
    '%_transient_%recovery%',
    '%_transient_%paused%'
));
echo "[4] Transients de error eliminados: $deleted\n";

/* 5. Resetear OPcache */
if (function_exists('opcache_reset')) {
    /* OPcache en CLI no tiene efecto sobre Apache. Necesitamos invalidar archivos clave */
    $archivos = [
        '/var/www/html/wp-content/themes/glorytemplate/vendor/composer/autoload_psr4.php',
        '/var/www/html/wp-content/themes/glorytemplate/vendor/autoload.php',
        '/var/www/html/wp-content/themes/glorytemplate/vendor/composer/autoload_real.php',
        '/var/www/html/wp-content/themes/glorytemplate/vendor/composer/autoload_static.php',
        '/var/www/html/wp-content/themes/glorytemplate/functions.php',
        '/var/www/html/wp-content/themes/glorytemplate/Glory/load.php',
        '/var/www/html/wp-content/themes/glorytemplate/App/Config/api.php',
        '/var/www/html/wp-content/themes/glorytemplate/App/Seo/CrestaSeo.php',
    ];
    $invalidated = 0;
    foreach ($archivos as $f) {
        if (file_exists($f) && opcache_invalidate($f, true)) {
            $invalidated++;
        }
    }
    opcache_reset();
    echo "[5] OPcache: reset + $invalidated archivos invalidados (nota: CLI OPcache != Apache OPcache)\n";
} else {
    echo "[5] OPcache no disponible en CLI\n";
}

echo "\n[6] Verificacion post-limpieza:\n";
echo "  recovery_keys: " . var_export(get_option('recovery_keys'), true) . "\n";
echo "  recovery_mode_email_last_sent: " . var_export(get_option('recovery_mode_email_last_sent'), true) . "\n";
echo "  _paused_themes: " . var_export(get_option('_paused_themes'), true) . "\n";
echo "  _paused_plugins: " . var_export(get_option('_paused_plugins'), true) . "\n";

echo "\n=== LIMPIEZA COMPLETADA ===\n";
echo "IMPORTANTE: Reiniciar Apache para limpiar OPcache del proceso web.\n";
