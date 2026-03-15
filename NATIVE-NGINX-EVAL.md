# Evaluación de backend nativo con Nginx

## Conclusión corta

Sí, es viable añadir un backend de despliegue nativo con Nginx, systemd y Docker opcional, pero debe vivir en una capa separada del backend Coolify para no mezclar supuestos operativos.

## Arquitectura propuesta

- Backend actual: adaptador Coolify.
- Backend nuevo: adaptador NativeProvisioner.
- Contrato común: crear sitio, desplegar código, hacer backup, restore, health check, auditar VPS.

## Requisitos del backend nativo

1. Provisionar Nginx, PHP-FPM, base de datos y certificados.
2. Gestionar systemd units y plantillas de virtual host.
3. Reutilizar el mismo sistema de backups externos locales.
4. Reutilizar health checks y auditorías VPS.

## Riesgos

- Mayor superficie de soporte que Coolify.
- Más responsabilidad sobre hardening del host.
- Necesidad de plantillas por distro y versión.

## Recomendación

Implementarlo solo después de extraer interfaces para SSH, HTTP, runtime de stack y provisionado. No mezclar lógica Nginx dentro de comandos actuales de Coolify.