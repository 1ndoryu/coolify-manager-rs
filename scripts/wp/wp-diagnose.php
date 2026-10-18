<?php
/*
 * Diagnostico WordPress — script reutilizable para coolify-manager run-script.
 * Muestra errores PHP, estado del tema, BD, y configuracion critica.
 */

error_reporting(E_ALL);
ini_set('display_errors', '1');
ini_set('max_execution_time', '120');

echo "=== DIAGNOSTICO WORDPRESS ===\n\n";

/* 1. Verificar archivos criticos */
echo "[1] Archivos criticos:\n";
$archivos = [
    '/var/www/html/wp-config.php',
    '/var/www/html/wp-load.php',
    '/var/www/html/wp-settings.php',
    '/var/www/html/.htaccess',
];
foreach ($archivos as $f) {
    $existe = file_exists($f) ? 'OK' : 'FALTA';
    echo "  $f: $existe\n";
}

/* 2. Verificar tema */
echo "\n[2] Tema activo:\n";
$themeDir = '/var/www/html/wp-content/themes/glorytemplate';
echo "  Directorio tema: " . (is_dir($themeDir) ? 'OK' : 'FALTA') . "\n";
echo "  functions.php: " . (file_exists("$themeDir/functions.php") ? 'OK' : 'FALTA') . "\n";
echo "  Glory/: " . (is_dir("$themeDir/Glory") ? 'OK' : 'FALTA') . "\n";
echo "  vendor/autoload.php: " . (file_exists("$themeDir/vendor/autoload.php") ? 'OK' : 'FALTA') . "\n";
echo "  .env: " . (file_exists("$themeDir/.env") ? 'OK' : 'FALTA') . "\n";

if (file_exists("$themeDir/.env")) {
    $envContent = file_get_contents("$themeDir/.env");
    $hexDump = bin2hex(substr($envContent, 0, 40));
    echo "  .env hex (primeros 40 bytes): $hexDump\n";
    echo "  .env contenido: " . trim($envContent) . "\n";
}

/* 3. Verificar autoload */
echo "\n[3] Autoload PSR-4:\n";
$autoloadFile = "$themeDir/vendor/composer/autoload_psr4.php";
if (file_exists($autoloadFile)) {
    $map = include($autoloadFile);
    foreach ($map as $ns => $paths) {
        echo "  $ns => " . implode(', ', $paths) . "\n";
    }
} else {
    echo "  autoload_psr4.php NO EXISTE\n";
}

/* 4. Conectar BD */
echo "\n[4] Conexion BD:\n";
$wpConfig = file_get_contents('/var/www/html/wp-config.php');
preg_match("/define.*DB_NAME.*['\"](.+?)['\"]/", $wpConfig, $m);
$dbName = $m[1] ?? '?';
preg_match("/define.*DB_USER.*['\"](.+?)['\"]/", $wpConfig, $m);
$dbUser = $m[1] ?? '?';
preg_match("/define.*DB_HOST.*['\"](.+?)['\"]/", $wpConfig, $m);
$dbHost = $m[1] ?? '?';

echo "  Host: $dbHost, User: $dbUser, DB: $dbName\n";

/* 5. Intentar cargar WP y capturar errores */
echo "\n[5] Cargando WordPress...\n";

/* Capturar fatal errors */
register_shutdown_function(function() {
    $error = error_get_last();
    if ($error && in_array($error['type'], [E_ERROR, E_PARSE, E_CORE_ERROR, E_COMPILE_ERROR])) {
        echo "\n!!! FATAL ERROR !!!\n";
        echo "  Tipo: " . $error['type'] . "\n";
        echo "  Mensaje: " . $error['message'] . "\n";
        echo "  Archivo: " . $error['file'] . "\n";
        echo "  Linea: " . $error['line'] . "\n";
    }
    echo "\n=== FIN DIAGNOSTICO ===\n";
});

/* Custom error handler para warnings/notices */
set_error_handler(function($errno, $errstr, $errfile, $errline) {
    $tipos = [
        E_WARNING => 'WARNING',
        E_NOTICE => 'NOTICE',
        E_DEPRECATED => 'DEPRECATED',
        E_USER_WARNING => 'USER_WARNING',
        E_USER_NOTICE => 'USER_NOTICE',
    ];
    $tipo = $tipos[$errno] ?? "ERROR($errno)";
    echo "  [$tipo] $errstr en $errfile:$errline\n";
    return true;
});

/* Definir constantes minimas para WP */
define('ABSPATH', '/var/www/html/');
define('WPINC', 'wp-includes');

/* Cargar wp-config.php directamente para obtener constantes DB */
$_SERVER['HTTP_HOST'] = 'localhost';
$_SERVER['REQUEST_URI'] = '/';

ob_start();
try {
    require_once('/var/www/html/wp-load.php');
    ob_end_clean();
    echo "  WordPress cargado OK\n";

    /* Verificar tema activo */
    echo "  Tema activo: " . get_option('template') . " / " . get_option('stylesheet') . "\n";
    echo "  Site URL: " . get_option('siteurl') . "\n";
    echo "  Home URL: " . get_option('home') . "\n";

    $plugins = get_option('active_plugins');
    echo "  Plugins activos: " . (is_array($plugins) ? implode(', ', $plugins) : 'ninguno') . "\n";

} catch (\Throwable $e) {
    ob_end_clean();
    echo "\n!!! EXCEPCION !!!\n";
    echo "  Clase: " . get_class($e) . "\n";
    echo "  Mensaje: " . $e->getMessage() . "\n";
    echo "  Archivo: " . $e->getFile() . "\n";
    echo "  Linea: " . $e->getLine() . "\n";
    echo "  Trace:\n";
    foreach (array_slice($e->getTrace(), 0, 10) as $i => $frame) {
        $file = $frame['file'] ?? '?';
        $line = $frame['line'] ?? '?';
        $fn = ($frame['class'] ?? '') . ($frame['type'] ?? '') . ($frame['function'] ?? '?');
        echo "    #$i $file:$line $fn()\n";
    }
}
