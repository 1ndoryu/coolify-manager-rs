#!/bin/bash
# Instala dependencias y ejecuta el build de React para Glory
# Requiere node y npm ya instalados en el contenedor

set -e

THEME_DIR="/var/www/html/wp-content/themes/glorytemplate"

echo "=== BUILD REACT GLORY ==="
echo ""

cd "$THEME_DIR"

echo "[1/4] Verificando node y npm..."
node --version
npm --version

echo ""
echo "[2/4] Instalando dependencias raiz..."
npm install --no-audit --no-fund 2>&1 | tail -5

echo ""
echo "[3/4] Verificando subdependencias..."
# El postinstall del root ya instala Glory/assets/react y App/React
# Pero por si acaso:
if [ ! -d "Glory/assets/react/node_modules" ]; then
    echo "Instalando Glory/assets/react deps..."
    npm install --prefix Glory/assets/react --no-audit --no-fund 2>&1 | tail -5
fi
if [ ! -d "App/React/node_modules" ]; then
    echo "Instalando App/React deps..."
    npm install --prefix App/React --no-audit --no-fund 2>&1 | tail -5
fi

echo ""
echo "[4/4] Ejecutando build..."
npm run build 2>&1

echo ""
echo "=== BUILD COMPLETADO ==="

# Verificar que se genero el manifest
MANIFEST="Glory/assets/react/dist/manifest.json"
if [ -f "$MANIFEST" ]; then
    echo "manifest.json OK ($(wc -c < $MANIFEST) bytes)"
    echo "Archivos en dist/:"
    ls -la Glory/assets/react/dist/ | head -20
else
    echo "ERROR: manifest.json NO se genero"
    ls -la Glory/assets/react/dist/ 2>/dev/null || echo "dist/ no existe"
    exit 1
fi
