# Plan detallado coolify-manager-rs

## Objetivo general

Convertir coolify-manager-rs en un gestor agnóstico de stacks con operaciones seguras de backup, restore, migración, health checks post-update, observabilidad MCP y auditoría operativa de VPS.

## Principios de implementación

- Ninguna operación destructiva sin snapshot verificable previo.
- Todo backup debe generarse fuera de la VPS objetivo.
- Toda restauración debe rechazar artefactos incompletos o corruptos.
- Las capacidades del stack se resuelven por abstracciones, no por ifs ad hoc en comandos.
- Toda operación crítica debe producir un manifiesto auditable y pruebas automáticas.

## Fase 1. Base agnóstica del dominio

1. Crear un modelo de capacidades por stack:
   - tipo de base de datos
   - estrategia de export/import de archivos
   - estrategia de health checks
   - estrategia de recarga del servidor web
2. Extraer la lógica hardcodeada de WordPress, Apache, Glory y MariaDB a adaptadores.
3. Añadir configuración extendida por sitio:
   - proveedor/stack
   - política de backups
   - opciones de health checks
   - metadatos de migración

## Fase 2. Sistema de backups externos por sitio

1. Añadir comando de snapshot unificado que capture:
   - metadatos del sitio
   - dump de base de datos
   - archivos persistentes del contenedor/volumen
   - estado git opcional
2. Persistir cada snapshot con estructura:
   - backups/{sitio}/daily/{timestamp}
   - backups/{sitio}/weekly/{timestamp}
   - manifest.json
   - checksums.sha256
   - estado.json
3. Implementar validación fuerte:
   - checksums
   - tamaños mínimos
   - presencia de archivos obligatorios
   - comprobación del dump antes de marcarlo como listo
4. Aplicar retención configurable por sitio:
   - daily keep 2
   - weekly keep 2
   - sin borrar backups con estado incompleto hasta auditar

## Fase 3. Restore seguro

1. Listar backups disponibles por sitio y tipo.
2. Restaurar solo desde snapshots en estado ready.
3. Ejecutar restore transaccional:
   - snapshot previo del estado actual
   - restore archivos
   - restore base de datos
   - ajuste de secretos y variables si aplica
   - health checks finales
4. Si falla el restore:
   - revertir usando snapshot previo
   - marcar incidente con causa y evidencia

## Fase 4. Migración VPS a VPS

1. Añadir definición de VPS destino en configuración.
2. Implementar pipeline de migración:
   - preflight en origen y destino
   - snapshot consistente del origen
   - transferencia local/externa del snapshot
   - provisionado del stack en destino
   - restore en destino
   - validaciones HTTP, DB y contenedores
   - conmutación controlada
3. Añadir modo dry-run y plan de ejecución.
4. Cubrir con tests de manifiesto, preflight y rollback.

## Fase 5. Updates protegidos y detección de errores

1. Crear flujo update-protected.
2. Antes del update:
   - snapshot
   - captura de commit git actual
   - health baseline
3. Después del update:
   - health checks HTTP
   - wp-cli o equivalente según stack
   - lectura de logs y señales fatales
4. Si falla:
   - ofrecer rollback git
   - ofrecer restore del último snapshot válido

## Fase 6. MCP y guía operativa

1. Exponer tools MCP para:
   - backups_create
   - backups_list
   - backups_restore
   - site_migrate
   - site_health_check
   - vps_audit
2. Documentar instalación en VS Code y pruebas manuales.
3. Añadir ejemplos de prompts y flujos recomendados.

## Fase 7. Rendimiento, seguridad y operación VPS

1. Auditoría remota de VPS:
   - carga CPU/RAM/disco
   - saturación de I/O
   - puertos expuestos
   - estado de Docker/Coolify
   - estado de firewall y fail2ban si existe
2. Recomendaciones automatizadas por hallazgos.
3. Evaluar servidor web alternativo por stack:
   - Caddy o Nginx cuando la imagen/stack lo soporte
   - mantener Apache solo donde sea requisito funcional

## Fase 8. Backend nativo sin Coolify

1. Diseñar un adaptador de despliegue separado del backend Coolify.
2. Definir provisión con Nginx nativo, systemd y backups equivalentes.
3. Mantener aislamiento total para no tocar la ruta actual basada en Coolify.

## Fase 9. Seguridad WordPress específica

1. Detectar credenciales admin débiles.
2. Añadir acciones guiadas para rotación de contraseñas y hardening básico.
3. Añadir auditoría de plugins críticos, debug expuesto y permisos inseguros.

## Fase 10. Evaluación de GUI Windows

1. Redactar md de viabilidad para Tauri + React.
2. Definir alcance mínimo: backups, restore, migración, health y auditorías.
3. Si la viabilidad es alta, levantar shell mínima desacoplada del CLI.

## Fase 11. Harness de tests offline

1. Introducir interfaces para SSH, Docker, HTTP y Coolify API.
2. Crear mocks deterministas de operaciones remotas.
3. Cubrir con tests sin VPS real: backup, restore, health, migración, update protegido y auditoría.

## Entregables

- CLI con comandos nuevos de backup, restore, migrate, health y audit.
- MCP actualizado con tools y recursos nuevos.
- Tests unitarios y de integración para manifiestos, retención, restore y migración.
- README actualizado y guía específica de MCP.
- mision.md actualizado con progreso y aprendizajes.

## Orden de ejecución

1. Base agnóstica mínima para operaciones de sitio.
2. Backups y restore seguros.
3. Migración entre VPS.
4. Update protegido.
5. MCP y documentación.
6. Auditoría VPS y optimizaciones.
7. Backend nativo separado.
8. Seguridad WordPress.
9. Evaluación GUI.
10. Harness de tests offline.