#!/usr/bin/env node
/* lock-generator.mjs (router) — NO contiene lógica del gate: delega en el adapter único
 * del área. [por que] Este archivo estaba copiado en 9 proyectos y cada fix había
 * que aplicarlo tantas veces (109A-9). Si necesitas cambiar el comportamiento,
 * cambia el adapter, no este router: se propaga con `quality:bump --shims --write`.
 */
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const ADAPTER = 'lock-generator.mjs';
const RELATIVO = path.join('workspace-manager', 'scripts', 'quality', ADAPTER);
const AREA_POR_DEFECTO = 'C:/Users/Owner/OneDrive/Documentos/area-trabajo';

/* Resuelve la raíz del área sin depender de una ruta absoluta frágil: variable
 * explícita, luego el propio manifiesto (sourcePath apunta a <área>/.quality-tools),
 * y solo como último recurso el default del área de trabajo. */
function raizDelArea(proyecto) {
  const porEntorno = (process.env.WS_AREA_ROOT ?? '').trim();
  if (porEntorno) return path.resolve(porEntorno);
  try {
    const manifest = JSON.parse(fs.readFileSync(path.join(proyecto, 'quality-tools.json'), 'utf8'));
    for (const tool of Object.values(manifest?.tools ?? {})) {
      if (typeof tool?.sourcePath !== 'string') continue;
      const absoluta = path.resolve(proyecto, tool.sourcePath);
      const corte = absoluta.toLowerCase().lastIndexOf(path.sep + '.quality-tools');
      if (corte > 0) return absoluta.slice(0, corte);
    }
  } catch { /* sin manifiesto legible: se usa el default del área */ }
  return AREA_POR_DEFECTO;
}

const proyecto = process.cwd();
const adapter = path.join(raizDelArea(proyecto), RELATIVO);
if (!fs.existsSync(adapter)) {
  process.stderr.write('[quality:lock] router: falta el adapter único del área en ' + adapter + '\n');
  process.stderr.write('[quality:lock] router: define WS_AREA_ROOT o provisiona el área; no se simula evidencia\n');
  process.exit(2);
}
/* Guard anti-recursión: si el destino resuelve al propio router, delegar sería
 * llamarse a sí mismo en bucle. [por que] Pasó cuando el adapter canónico fue
 * sobrescrito por un router; el fallo se veía como error de job object, no como
 * recursión, y costó diagnosticarlo. Mejor un error explícito. */
if (path.resolve(adapter) === path.resolve(process.argv[1] ?? '')) {
  process.stderr.write('[quality:lock] router: el adapter resuelve al propio router (' + adapter + ')\n');
  process.stderr.write('[quality:lock] router: restaura el adapter canónico; no se delega en sí mismo\n');
  process.exit(2);
}
const resultado = spawnSync(process.execPath, [adapter, ...process.argv.slice(2)], {
  cwd: proyecto,
  stdio: 'inherit',
  windowsHide: true,
});
if (resultado.error) {
  process.stderr.write('[quality:lock] router: no se pudo ejecutar el adapter: ' + resultado.error.message + '\n');
  process.exit(2);
}
process.exit(resultado.status ?? 2);
