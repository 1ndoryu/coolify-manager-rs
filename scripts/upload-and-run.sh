#!/bin/bash
# Sube el script de diagnostico al web root y lo hace accesible via HTTP
SCRIPT_B64="PLACEHOLDER"

echo "$SCRIPT_B64" | base64 -d > /var/www/html/cm-diagnose.php
chown www-data:www-data /var/www/html/cm-diagnose.php
chmod 644 /var/www/html/cm-diagnose.php
echo "Script subido a /var/www/html/cm-diagnose.php"
ls -la /var/www/html/cm-diagnose.php

# Hacer request HTTP local para obtener el diagnostico
curl -s http://localhost/cm-diagnose.php 2>&1
echo ""
echo "=== Script se autoelimina despues de ejecutarse ==="
