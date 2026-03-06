#!/bin/bash
# Lee los ultimos errores del log de Docker (stderr de Apache/PHP) 
# y verifica el estado real de Apache

echo "=== APACHE ERROR LOG ==="
cat /var/log/apache2/error.log 2>/dev/null | tail -40 || echo "No apache error.log"

echo ""
echo "=== PHP ERROR LOG (stderr) ==="
# En contenedores Docker oficiales de WP, los errores PHP van a stderr
# que Docker captura. Pero podemos verificar si hay archivo

echo ""
echo "=== PHP INFO RELEVANTE ==="
php -r "echo 'CLI max_execution_time: ' . ini_get('max_execution_time') . PHP_EOL;"
# Verificar la config de Apache/mod_php
php -r "
\$ini_files = [
    '/usr/local/etc/php/php.ini',
    '/usr/local/etc/php/conf.d/',
];
foreach (\$ini_files as \$f) {
    if (is_file(\$f)) echo \"INI: \$f exists\\n\";
    if (is_dir(\$f)) {
        foreach (glob(\$f . '*.ini') as \$ini) {
            echo \"INI: \$ini\\n\";
        }
    }
}
echo 'Apache max_exec: ';
// Simular SAPI condition
echo ini_get('max_execution_time') . PHP_EOL;
"

echo ""
echo "=== WP RECOVERY/PAUSED STATE ==="
php -r "
define('ABSPATH', '/var/www/html/');
\$_SERVER['HTTP_HOST'] = 'localhost';
\$_SERVER['REQUEST_URI'] = '/';
require_once('/var/www/html/wp-load.php');

// Check for stored errors
\$key = 'wp_fatal_error_handler_enabled';
echo \"\$key: \" . var_export(get_option(\$key), true) . \"\\n\";

// Check paused extensions
echo '_paused_themes: ' . var_export(get_option('_paused_themes'), true) . \"\\n\";
echo '_paused_plugins: ' . var_export(get_option('_paused_plugins'), true) . \"\\n\";

// Check recovery keys
global \$wpdb;
\$rows = \$wpdb->get_results(\"SELECT option_name, LEFT(option_value, 100) AS val FROM wp_options WHERE option_name LIKE '%recovery%' OR option_name LIKE '%paused%' OR option_name LIKE '%fatal%' OR option_name LIKE '%error%'\");
echo \"\\n=== BD recovery/error options ===\\n\";
foreach (\$rows as \$r) {
    echo \"\$r->option_name = \$r->val\\n\";
}

// Simulate a homepage query like Apache would
echo \"\\n=== SIMULAR WP_QUERY HOME ===\\n\";
\$t = microtime(true);
\$q = new WP_Query(['posts_per_page' => 10, 'post_type' => 'page', 'post_status' => 'publish']);
\$elapsed = round((microtime(true) - \$t) * 1000);
echo \"WP_Query HOME: {\$q->post_count} posts en {\$elapsed}ms\\n\";
echo \"Total posts found: {\$q->found_posts}\\n\";

// Check if theme template exists
\$template_file = get_template_directory() . '/index.php';
echo \"\\nTemplate index.php: \" . (file_exists(\$template_file) ? 'OK' : 'FALTA') . \"\\n\";
echo \"Content: \" . substr(file_get_contents(\$template_file), 0, 200) . \"\\n\";
"
