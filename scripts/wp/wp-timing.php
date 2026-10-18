<?php
/*
 * Diagnostico de timing — mide cuanto tarda cada fase del boot de WordPress.
 * Detecta timeouts y cuellos de botella.
 */

error_reporting(E_ALL);
ini_set('display_errors', '1');
ini_set('max_execution_time', '120');

$start = microtime(true);

function elapsed() {
    global $start;
    return round((microtime(true) - $start) * 1000);
}

echo "=== DIAGNOSTICO TIMING WP ===\n\n";

/* Simular request HTTP */
$_SERVER['HTTP_HOST'] = 'wordpress-x0w800c40wgww4k888gw8s8s.66.94.100.241.sslip.io';
$_SERVER['REQUEST_URI'] = '/';
$_SERVER['SERVER_PROTOCOL'] = 'HTTP/1.1';
$_SERVER['REQUEST_METHOD'] = 'GET';
$_SERVER['SERVER_NAME'] = $_SERVER['HTTP_HOST'];
$_SERVER['SERVER_PORT'] = '80';

echo "[" . elapsed() . "ms] Inicio\n";

/* Fase 1: wp-config.php */
define('ABSPATH', '/var/www/html/');

register_shutdown_function(function() {
    $error = error_get_last();
    if ($error && in_array($error['type'], [E_ERROR, E_PARSE, E_CORE_ERROR, E_COMPILE_ERROR])) {
        echo "\n!!! FATAL ERROR a " . elapsed() . "ms !!!\n";
        echo "  " . $error['message'] . "\n";
        echo "  " . $error['file'] . ":" . $error['line'] . "\n";
    }
    echo "\n[" . elapsed() . "ms] FIN TOTAL\n";
});

set_error_handler(function($errno, $errstr, $errfile, $errline) {
    if ($errno === E_WARNING || $errno === E_USER_WARNING) {
        echo "  [WARN " . elapsed() . "ms] $errstr ($errfile:$errline)\n";
    }
    return true;
});

/* Cargar wp-settings paso a paso */
echo "[" . elapsed() . "ms] Pre wp-load.php\n";

ob_start();
require_once('/var/www/html/wp-load.php');
$output = ob_get_clean();

echo "[" . elapsed() . "ms] Post wp-load.php\n";

if ($output) {
    echo "  Buffered output: " . strlen($output) . " bytes\n";
    if (strpos($output, 'error') !== false || strpos($output, 'Error') !== false) {
        echo "  Output contiene 'error': " . substr($output, 0, 500) . "\n";
    }
}

/* Verificar que el tema funciona */
echo "[" . elapsed() . "ms] Verificando tema...\n";
$template = get_option('template');
$stylesheet = get_option('stylesheet');
echo "  template=$template, stylesheet=$stylesheet\n";

/* Verificar si hay errores fatales guardados */
$recovery = get_option('wp_fatal_error_handler_enabled');
$paused = get_option('_paused_themes');
$pausedPlugins = get_option('_paused_plugins');

echo "\n[RECOVERY MODE]:\n";
echo "  fatal_error_handler_enabled: " . var_export($recovery, true) . "\n";
echo "  _paused_themes: " . var_export($paused, true) . "\n";
echo "  _paused_plugins: " . var_export($pausedPlugins, true) . "\n";

/* Verificar recovery mode data */
if (function_exists('wp_get_fatal_error_handler')) {
    echo "  wp_get_fatal_error_handler disponible\n";
}

/* Verificar opciones de recovery */
$recoveryData = get_option('recovery_mode_email_last_sent');
echo "  recovery_mode_email_last_sent: " . var_export($recoveryData, true) . "\n";

/* Verificar error del tema en la DB — WP guarda el error aqui */
$themeError = get_option('theme_switch_error');
echo "  theme_switch_error: " . var_export($themeError, true) . "\n";

/* Verificar PHP fatal error log */
$wpDebugLog = '/var/www/html/wp-content/debug.log';
if (file_exists($wpDebugLog)) {
    $lines = array_slice(file($wpDebugLog), -20);
    echo "\n[DEBUG.LOG ultimas 20 lineas]:\n";
    foreach ($lines as $line) {
        echo "  " . trim($line) . "\n";
    }
}

/* PHP error log del sistema */
$phpLog = ini_get('error_log');
echo "\n[PHP error_log]: $phpLog\n";
if ($phpLog && file_exists($phpLog)) {
    $lines = array_slice(file($phpLog), -20);
    echo "[Ultimas 20 lineas]:\n";
    foreach ($lines as $line) {
        echo "  " . trim($line) . "\n";
    }
}

/* Verificar apache error log */
$apacheLogs = ['/var/log/apache2/error.log', '/var/log/apache2/error_log'];
foreach ($apacheLogs as $log) {
    if (file_exists($log)) {
        $lines = array_slice(file($log), -20);
        echo "\n[APACHE LOG: $log]:\n";
        foreach ($lines as $line) {
            echo "  " . trim($line) . "\n";
        }
    }
}

/* Verificar max_execution_time */
echo "\n[PHP Config]:\n";
echo "  max_execution_time: " . ini_get('max_execution_time') . "\n";
echo "  memory_limit: " . ini_get('memory_limit') . "\n";
echo "  PHP version: " . PHP_VERSION . "\n";
echo "  PHP SAPI: " . PHP_SAPI . "\n";
