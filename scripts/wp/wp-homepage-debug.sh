#!/bin/bash
# Hace curl a la pagina principal de WordPress (pasa por .htaccess + index.php + tema)
# y tambien captura el docker error log para ver PHP fatals

echo "=== CURL A / (homepage) ==="
RESPONSE=$(curl -s --max-time 60 -w "\n\nHTTP_CODE:%{http_code}\nTIME:%{time_total}" http://localhost/ 2>&1)
echo "$RESPONSE" | tail -30

echo ""
echo "=== CURL COMPLETO A /wp-admin/ ==="
ADMIN_RESPONSE=$(curl -sI --max-time 30 http://localhost/wp-admin/ 2>&1)
echo "$ADMIN_RESPONSE"

echo ""
echo "=== CURL A /wp-login.php ==="
LOGIN_RESPONSE=$(curl -s --max-time 30 -w "\nHTTP_CODE:%{http_code}" http://localhost/wp-login.php 2>&1)
echo "$LOGIN_RESPONSE" | head -20
echo "..."
echo "$LOGIN_RESPONSE" | tail -5

echo ""
echo "=== PHP ERROR LOG (ultimas 30 lineas stderr) ==="
# El error_log de PHP en Docker da a stderr, que no se puede leer desde dentro.
# Pero podemos verificar el ini y probar con archivo temporal

# Crear una config para loggear a archivo temporalmente
cat > /usr/local/etc/php/conf.d/zz-debug-temp.ini << 'INI'
error_reporting = E_ALL
display_errors = Off
log_errors = On
error_log = /tmp/php_debug.log
max_execution_time = 120
INI

echo "Config temporal creada. Reiniciando Apache..."
apache2ctl graceful 2>&1 | grep -v "Could not reliably"
sleep 2

echo "Haciendo request despues del reinicio..."
truncate -s 0 /tmp/php_debug.log 2>/dev/null
touch /tmp/php_debug.log
chmod 666 /tmp/php_debug.log

curl -s --max-time 60 http://localhost/ > /dev/null 2>&1

echo ""
echo "=== PHP ERRORS LOG ==="
cat /tmp/php_debug.log 2>/dev/null

# Limpiar config temporal
rm -f /usr/local/etc/php/conf.d/zz-debug-temp.ini
apache2ctl graceful 2>&1 | grep -v "Could not reliably"
