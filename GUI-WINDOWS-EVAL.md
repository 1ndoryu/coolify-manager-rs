# Evaluación GUI Windows con Tauri + React

## Conclusión corta

Sí, es viable. El CLI actual ya tiene suficiente superficie funcional para servir como core de una GUI mínima en Windows usando Tauri + React.

## Estado actual — mayo 2026

La GUI ya arranca con `npm run tauri dev` desde `gui/` y consume comandos Tauri estructurados (`list_sites`, `health_check`, `list_backups`). La vista Vite en navegador también es utilizable: carga datos demo, muestra estados por fila y permite revisar el diseño sin runtime nativo.

La experiencia principal ya no se divide en vistas sueltas de Salud/Backups/Auditoría. El modelo actual es una consola table-first: cada sitio muestra status inline, acciones por fila y backups contextuales. Logs, restart, redeploy, restore y backup manual quedan como Fase 2 con confirmaciones.

`settings.json` se resuelve desde ruta explícita, `COOLIFY_MANAGER_CONFIG`, ancestros del directorio actual, `CARGO_MANIFEST_DIR` y ancestros del ejecutable. Esto evita el fallo de Windows cuando `CARGO_TARGET_DIR` ejecuta el binario desde `C:\tmp\glory-target`.

Para diagnosticar la ruta real:

```bash
cargo run -- get-config-path
```

## Alcance mínimo recomendado

1. Lista de sitios con estado por fila.
2. Backups contextuales por sitio.
3. Crear backup manual con confirmación.
4. Logs, restart y redeploy protegidos.
5. Restore de backup concreto con confirmación fuerte.
6. Migración en dry-run como flujo posterior.

## Arquitectura propuesta

- Core: binario Rust actual expuesto como comandos internos o librería.
- Shell: app Tauri.
- UI: React table-first con una consola de servicios, acciones por fila y paneles contextuales.

## Requisitos previos

- Estabilizar interfaces de servicios para no acoplar la GUI al parser CLI.
- Añadir respuestas estructuradas JSON en operaciones críticas.
- Completar harness de tests offline antes de abrir la GUI a operaciones destructivas.

## Recomendación

Proceder con una GUI mínima solo después de extraer una API interna compartida entre CLI y Tauri. No llamar al parser clap desde la GUI.