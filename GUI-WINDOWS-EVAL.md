# Evaluación GUI Windows con Tauri + React

## Conclusión corta

Sí, es viable. El CLI actual ya tiene suficiente superficie funcional para servir como core de una GUI mínima en Windows usando Tauri + React.

## Alcance mínimo recomendado

1. Lista de sitios.
2. Crear backup manual y listar backups.
3. Restore de backup concreto.
4. Health check.
5. Auditoría VPS.
6. Migración en dry-run.

## Arquitectura propuesta

- Core: binario Rust actual expuesto como comandos internos o librería.
- Shell: app Tauri.
- UI: React sencilla con vistas de Sitios, Backups, Salud, Migración y Auditoría.

## Requisitos previos

- Estabilizar interfaces de servicios para no acoplar la GUI al parser CLI.
- Añadir respuestas estructuradas JSON en operaciones críticas.
- Completar harness de tests offline antes de abrir la GUI a operaciones destructivas.

## Recomendación

Proceder con una GUI mínima solo después de extraer una API interna compartida entre CLI y Tauri. No llamar al parser clap desde la GUI.