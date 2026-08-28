# coolify-manager-rs — Roadmap

> **Descripción:** Herramienta de gestión para sitios Coolify — CLI + MCP Server + GUI web + portal vps.nakomi.studio
> **Stack:** Rust/Axum (backend) + React/Vite/TypeScript (frontend GUI)
> **Repositorio:** github.com/1ndoryu/coolify-manager-rs (rama `main`)
> **Deploy:** Coolify — requiere aprobación explícita del operador antes de ejecutar
> **Plan activo:** `Agente/planes/plan-vps-nakomi-studio-2026-05-12.md`

## Herramientas del agente
- coolify-manager-rs (este proyecto), code-sentinel, varsense (ver protocolo sección VII)

## Tareas pendientes

## Mejoras pendientes (268A-5, verificadas en despliegue real de agape)

- **E11 rollback ciego a HTTP:** `deploy-service` en un sitio NUEVO sin DNS configurado falla el health
  check HTTPS (`https://dominio/api/health` no resuelve) y entra en bucle rollback→rebuild (~10 min
  por ciclo) aunque el contenedor esté healthy y `/api/health` interno responda 200. Mejora:
  distinguir "dominio no resuelve aún" (warning, no rollback) de "app rota" (rollback). Idea:
  verificar resolución DNS del FQDN antes de tratar el fallo HTTP como fatal, o usar la URL interna
  (sslip.io / IP del contenedor) como health primario cuando el DNS del dominio aún no apunte.

## Mejora E12 (IMPLEMENTADA 2026-08-28): comando `db-compare` — comparación automática y precisa de BD

- **Motivación:** la verificación de pérdida de datos del incidente 27/08 se hizo comparando dumps
  SQL a mano (grep/awk/zcat). Método impreciso y frágil: INSERT multi-fila con contenido enorme,
  conteos de `(` falsos, tablas personalizadas desconocidas (pgvector, plugins WP, tipos custom) y
  spam que confunde el diff. El usuario pidió automatizarlo de forma segura y precisa, sin depender
  de conocer las tablas.
- **Solución implementada:** nuevo comando `db-compare` que descubre tablas automáticamente
  (`information_schema`/`SHOW TABLES`), y en modo completo restaura el dump en un **contenedor
  temporal efímero** (nunca toca la BD viva) y compara ambas BDs con SQL real (JSON por fila +
  comparación de conjuntos en Rust), sin parsear texto. Salida JSON estructurada: tablas solo-en-A,
  solo-en-B, idénticas, con diferencia + muestra limitada. Maneja pgvector, tablas sin PK, bytea y
  columnas volátiles. 100% solo lectura sobre la BD en vivo.
- **Seguridad/limpieza:** contenedor temporal `--rm --network none` + `docker rm -f` SIEMPRE (éxito
  o error); dumps subidos a `/tmp/dbcompare_*.sql` también se borran SIEMPRE; `cleanup_all_temp`
  barre contenedores y dumps huérfanos de ejecuciones abortadas. Verificado en producción: 0
  contenedores y 0 dumps residuales tras ejecuciones reales.
- **Verificado en producción (28/08):** agape (13 tablas idénticas, modo ligero), guillermo
  (MariaDB, 11 idénticas + wp_options con diffs de cron/transients), glory-rest (PG, 40/40
  idénticas = sin pérdida), studio (PG, 53 idénticas + 3 con diffs de telemetría/timestamps =
  sin pérdida de datos de negocio).
- **Bugs corregidos durante la implementación:** (1) `find_latest_vps_dump` no encontraba dumps
  MariaDB (`mariadb-{uuid}` vs `{uuid}`); (2) `JSON_OBJECT` de MariaDB se rompía por backticks
  interpretados como command substitution — ahora se envía por base64 (patrón `pg_utils`).
- **Plan detallado:** `Agente/planes/completados/plan-db-compare-2026-08-28.md` (completado).
- **Documentación:** `Agente/documentacion/incidente-backups-2026-08-27.md` (método manual
  reemplazado por db-compare) y sección de comandos del README.

## Incidente 2026-08-27: backups programados (dos sistemas — VPS OK, Windows roto) (DIAGNOSTICADO)

- **Contexto:** hay DOS sistemas de backups independientes.
- **Sistema VPS (OPERATIVO, canónico):** `/usr/local/bin/backup-server.sh` + crontab root `0 3 * * *`.
  Corre en el servidor, independiente del PC Windows. Log `/data/backups/backup.log` muestra
  `BACKUP RUN` diario con `errors=0` (16→27/08). Dumps `.sql.gz` por stack UUID con datos reales.
  **El dump de studio del 27/08 01:00 contiene todos los datos** (7 proyectos) — capturado antes del
  incidente de reinicialización (23:35). Es la **mejor fuente de restauración de studio**.
- **Sistema Windows (legacy, ROTO desde ~14/08):** Task Scheduler `CoolifyManager-Backup-*` apunta al
  binario legacy `...\glorytemplate\.agent\coolify-manager-rs\target\release\coolify-manager.exe`
  que **ya no existe**. El `LastTaskResult=0` es engañoso (`.bat` con auto-ocultamiento sale con 0 sin
  ejecutar el backup real). Último backup legacy: `20260813_*`. **Era redundante, no el único.**
- **Impacto:** `studio` (nakomi.studio) con BD viva pero PGDATA reinicializado 27/08 23:35; restaurar
  desde dump VPS 27/08 (principal) o legacy 13/08 (alternativa, incluye uploads). `kamples`,
  `glory-rest` y `agape` sin pérdida (VPS los respalda).
- **Detalle kamples:** falta extensión pgvector en el postgres recreado (`$libdir/vector`).
- **Documento completo:** `Agente/documentacion/incidente-backups-2026-08-27.md`
- **Pendientes (trabajo):**
  1. Confirmar cobertura del VPS para `task` (nuevo 27/08) en el próximo `BACKUP RUN`.
  2. Restaurar `studio` desde dump VPS 27/08 (requiere autorización — escritura de producción).
  3. Reinstalar pgvector en el postgres de kamples.
  4. Decidir destino del sistema legacy Windows (eliminar tareas/`.bat` o reparar apuntando al
     binario canónico); recomendado eliminarlo/documentarlo como obsoleto.

## Incidente 2026-08-27: limpieza global de contenedores exited (CORREGIDO, commit eb1ce73)

- **Síntoma:** el deploy de `task` (stack `j4skk8...`) tumbó los otros 9 sitios (5 WordPress + 4 Rust).
- **Causa raíz:** el bloque `[04A-1]` de `deploy_service.rs` limpiaba contenedores exited con
  `docker ps -a --filter status=exited` + `docker rm {name}` **sin filtrar por stack**, borrando
  contenedores de TODOS los sitios del host.
- **Impacto:** 9 sitios caídos (503) — contenedores borrados, datos intactos (docker rm no toca
  volúmenes ni imágenes). Ninguna pérdida de datos verificada.
- **Fix aplicado (commit `eb1ce73`):** la limpieza ahora filtra por
  `label=coolify.stack-uuid={uuid}` (mismo patrón que `diagnose.rs`), solo toca contenedores del
  stack objetivo. Test de regresión `cleanup_exited_cmd_filters_by_stack_uuid` añadido.
- **Lección:** toda operación de limpieza/búsqueda de contenedores en producción DEBE filtrar por
  `label=coolify.stack-uuid={uuid}`. Nunca `docker ps -a` global.
- **Pendiente opcional:** revisar `docker_host_cleanup_manager.rs` y `target_bootstrap_manager.rs`
  (también usan `docker ps -a` global) para confirmar que su alcance es intencional (limpieza
  explícita de host) y no un riesgo similar.

### Fase 2 — Deploy online (BLOQUEADO — requiere supervisión del operador)

- 105A-34: Despliegue `vps.nakomi.studio` — **NO ejecutar sin aprobación explícita del operador**
  - Prerrequisitos completados: 125A-1, 125A-2, 125A-3
  - Prerrequisito pendiente: revisión local por el operador

### Fase 3 — MVP online seguro (post-deploy)

- 105A-36: RBAC + auditoría — roles admin/operator/viewer, tabla de eventos
- 105A-42: API read-only con DTOs seguros — sin paths, tokens ni config cruda
- 105A-44: Permisos write + auditoría completa de eventos

### Fase 4 — Portal VPS (post-deploy)

- 105A-37: Portal VPS conectado a API de Nakomi — panel cliente + panel admin
