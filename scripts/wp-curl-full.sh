#!/bin/bash
# Diagnostico HTTP completo que replica lo que ve el navegador

HOST="wordpress-x0w800c40wgww4k888gw8s8s.66.94.100.241.sslip.io"

# Asegurar config de logging temporal
cat > /usr/local/etc/php/conf.d/zz-debug-temp.ini << 'INI'
error_reporting = E_ALL
display_errors = Off
log_errors = On
error_log = /tmp/php_debug.log
max_execution_time = 120
INI
apache2ctl graceful 2>&1 | grep -v "Could not reliably"
sleep 2
truncate -s 0 /tmp/php_debug.log 2>/dev/null
touch /tmp/php_debug.log
chmod 666 /tmp/php_debug.log

echo "=== CURL HOMEPAGE (con Host header, follow redirects) ==="
RESPONSE=$(curl -s -L --max-time 90 -H "Host: $HOST" -w "\n\nHTTP_CODE:%{http_code}\nTIME:%{time_total}\nREDIRECTS:%{num_redirects}" http://localhost/ 2>&1)

# Mostrar solo las partes relevantes
HTTP_CODE=$(echo "$RESPONSE" | grep "^HTTP_CODE:" | cut -d: -f2)
TIME=$(echo "$RESPONSE" | grep "^TIME:" | cut -d: -f2)
REDIRECTS=$(echo "$RESPONSE" | grep "^REDIRECTS:" | cut -d: -f2)

echo "HTTP Code: $HTTP_CODE"
echo "Time: ${TIME}s"
echo "Redirects: $REDIRECTS"
echo ""

# Mostrar body (sin metadata lines)
BODY=$(echo "$RESPONSE" | head -n -4)
BODY_LEN=${#BODY}
echo "Body length: $BODY_LEN bytes"
echo ""

if [ $BODY_LEN -lt 5000 ]; then
    echo "=== BODY COMPLETO ==="
    echo "$BODY"
else
    echo "=== BODY (primeros 2000 chars) ==="
    echo "$BODY" | head -c 2000
    echo ""
    echo "..."
    echo "=== BODY (ultimos 1000 chars) ==="
    echo "$BODY" | tail -c 1000
fi

echo ""
echo "=== PHP ERROR LOG ==="
cat /tmp/php_debug.log 2>/dev/null
if [ ! -s /tmp/php_debug.log ]; then
    echo "(vacio)"
fi

# Limpiar
rm -f /usr/local/etc/php/conf.d/zz-debug-temp.ini
apache2ctl graceful 2>&1 | grep -v "Could not reliably"
