#!/bin/bash
# Copia el script PHP de diagnostico al web root, lo ejecuta via curl (Apache SAPI), y limpia

# El script PHP ya fue colocado como /tmp/cm_script.sh por run-script, 
# pero necesitamos el PHP en el webroot. Lo recodificamos inline.

cat > /var/www/html/cm-diagnose.php << 'PHPEOF'
<?php
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

echo "\n[" . elapsed() . "ms] Verificaciones:\n";
echo "  template: " . get_option('template') . "\n";
echo "  siteurl: " . get_option('siteurl') . "\n";

global $wp_query;
if (isset($wp_query)) {
    echo "  wp_query post_count: " . $wp_query->post_count . "\n";
}

echo "\n[" . elapsed() . "ms] Todo OK bajo Apache SAPI\n";
PHPEOF

chown www-data:www-data /var/www/html/cm-diagnose.php
echo "Script colocado. Ejecutando via curl..."
echo ""

# Ejecutar via HTTP (Apache SAPI) con timeout largo
curl -s --max-time 90 http://localhost/cm-diagnose.php 2>&1
CURL_EXIT=$?

echo ""
if [ $CURL_EXIT -ne 0 ]; then
    echo "CURL FALLO con exit code $CURL_EXIT"
    # Si curl fallo, verificar si el script sigue ahi (no se autoelimino = no termino)
    if [ -f /var/www/html/cm-diagnose.php ]; then
        echo "El script no se completo (timeout probable). Limpiando..."
        rm -f /var/www/html/cm-diagnose.php
    fi
fi
