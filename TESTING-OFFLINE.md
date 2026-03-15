# Estrategia de tests offline

## Objetivo

Validar backup, restore, health, migración y auditoría sin depender de VPS reales, Docker real ni Coolify real.

## Diseño recomendado

1. Extraer traits para SSH, Docker, HTTP y Coolify API.
2. Inyectar implementaciones reales en producción y mocks en tests.
3. Modelar fixtures de contenedores, archivos y respuestas HTTP.

## Casos mínimos a cubrir

1. Backup listo con manifiesto correcto y retención aplicada.
2. Restore rechaza backup incompleto o con checksum inválido.
3. Update protegido hace rollback si el health check falla.
4. Migración dry-run produce plan sin tocar destino.
5. Migración real revierte si falla el health check final.
6. Auditoría VPS interpreta correctamente estados de carga, disco y seguridad.

## Estado actual

- Hay tests unitarios para utilidades y dominio.
- Aún falta introducir interfaces mockeables para cubrir flujos remotos completos.

## Prioridad

Alta. Esta capa es el prerrequisito real para cerrar QM10 con seguridad.