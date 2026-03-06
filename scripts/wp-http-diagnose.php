<?php
/*
 * Diagnostico que corre bajo Apache (acceder via HTTP, no CLI).
 * Coloca en /var/www/html/cm-diagnose.php y accede via navegador.
 * SE AUTOELIMINA al final por seguridad.
 */

error_reporting(E_ALL);
ini_set('display_errors', '1');
ini_set('log_errors', '1');
ini_set('max_execution_time', 120);

header('Content-Type: text/plain; charset=utf-8');

$start = microtime(true);
function elapsed() { global $start; return round((microtime(true) - $start) * 1000); }

echo "=== DIAGNOSTICO APACHE (SAPI: " . PHP_SAPI . ") ===\n\n";
echo "PHP " . PHP_VERSION . "\n";
echo "max_execution_time: " . ini_get('max_execution_time') . "\n";
echo "memory_limit: " . ini_get('memory_limit') . "\n";
echo "opcache.enable: " . ini_get('opcache.enable') . "\n\n";

register_shutdown_function(function() {
    $error = error_get_last();
    if ($error && in_array($error['type'], [E_ERROR, E_PARSE, E_CORE_ERROR, E_COMPILE_ERROR])) {
        echo "\n!!! FATAL ERROR a " . elapsed() . "ms !!!\n";
        echo "Tipo: " . $error['type'] . "\n";
        echo "Mensaje: " . $error['message'] . "\n";
        echo "Archivo: " . $error['file'] . "\n";
        echo "Linea: " . $error['line'] . "\n";
    }
    echo "\n[" . elapsed() . "ms] FIN\n";
    /* Autoeliminar por seguridad */
    @unlink(__FILE__);
});

set_error_handler(function($errno, $errstr, $errfile, $errline) {
    $tipos = [E_WARNING => 'WARN', E_NOTICE => 'NOTICE', E_DEPRECATED => 'DEPRECATED'];
    $t = $tipos[$errno] ?? "E$errno";
    echo "  [$t " . elapsed() . "ms] $errstr ($errfile:$errline)\n";
    return true;
});

echo "[" . elapsed() . "ms] Pre wp-load\n";

try {
    define('ABSPATH', '/var/www/html/');
    require_once('/var/www/html/wp-load.php');
    echo "[" . elapsed() . "ms] Post wp-load OK\n";
} catch (\Throwable $e) {
    echo "[" . elapsed() . "ms] EXCEPCION en wp-load:\n";
    echo "  " . get_class($e) . ": " . $e->getMessage() . "\n";
    echo "  " . $e->getFile() . ":" . $e->getLine() . "\n";
    foreach (array_slice($e->getTrace(), 0, 15) as $i => $f) {
        echo "  #$i " . ($f['file'] ?? '?') . ":" . ($f['line'] ?? '?') . " " . ($f['class'] ?? '') . ($f['type'] ?? '') . ($f['function'] ?? '') . "\n";
    }
    exit;
}

echo "\n[" . elapsed() . "ms] Verificaciones post-boot:\n";
echo "  template: " . get_option('template') . "\n";
echo "  stylesheet: " . get_option('stylesheet') . "\n";
echo "  siteurl: " . get_option('siteurl') . "\n";

echo "\n[" . elapsed() . "ms] Simulando template load...\n";

/* Simular lo que WP hace al renderizar una pagina */
try {
    $templateDir = get_template_directory();
    echo "  template_dir: $templateDir\n";

    /* Verificar que el tema se puede cargar completamente */
    $themeRoot = get_theme_root() . '/' . get_template();
    echo "  theme_root: $themeRoot\n";

    /* Ejecutar query principal como lo haria WP */
    global $wp_query;
    if (isset($wp_query)) {
        echo "  wp_query existe, post_count: " . $wp_query->post_count . "\n";
        echo "  is_home: " . var_export($wp_query->is_home(), true) . "\n";
        echo "  is_front_page: " . var_export($wp_query->is_front_page(), true) . "\n";
    }

    /* Simular template include */
    $template_file = get_index_template();
    echo "  Template file resolved: $template_file\n";

    echo "\n[" . elapsed() . "ms] Todo OK\n";

} catch (\Throwable $e) {
    echo "  EXCEPCION: " . $e->getMessage() . "\n";
    echo "  " . $e->getFile() . ":" . $e->getLine() . "\n";
    foreach (array_slice($e->getTrace(), 0, 10) as $i => $f) {
        echo "  #$i " . ($f['file'] ?? '?') . ":" . ($f['line'] ?? '?') . " " . ($f['class'] ?? '') . ($f['type'] ?? '') . ($f['function'] ?? '') . "\n";
    }
}
