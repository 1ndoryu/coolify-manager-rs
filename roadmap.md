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
- **Verificado en producción (28/08, TODOS LOS SITIOS):** verificación final 10/10 sitios contra
  el último dump VPS (28/08 01:00): agape 13/13, glory-rest 40/40, task 23/23, kamples 39/40
  (solo timestamp `ultimo_rapido` + tabla `samples` nueva post-dump), nakomi 38/39, wandori 13/14,
  cap 20/21, padel 25/27 (1 comentario spam nuevo en vivo post-dump), guillermo 11/12, studio
  53/56 (solo telemetría/timestamps). **VEREDICTO: SIN PÉRDIDA DE DATOS en ningún sitio.** Todos
  los diffs son timestamps/transients volátiles, filas nuevas en vivo o tablas creadas post-dump.
- **⚠️ CORRECCIÓN 05/09 (falso negativo):** el veredicto «studio SIN PÉRDIDA» del 28/08 era
  **incorrecto**: comparaba contra el dump 27/08 (que SÍ tenía datos), enmascarando un vaciado
  posterior de la capa de negocio. El 05/09 se confirmó projects=0/users=0/orders=0 en vivo
  (pérdida real entre 23/08 y 30/08) y se restauró. Lección: `db-compare` «sin pérdida» vs un
  dump con datos NO prueba ausencia de vaciado posterior a ese dump; comparar también conteos
  vivos de tablas de negocio clave contra valores esperados. Detalle en §0 del documento del incidente.
- **Fix de charset durante la verificación (commit `f68a53f`):** el cliente `mariadb` del
  contenedor vivo devolvía emojis UTF-8 de 4 bytes como `?` (falsos positivos) → añadido
  `--default-character-set=utf8mb4` a la extracción. nakomi pasó de 27/39 a 38/39 idénticas.
- **Bugs corregidos durante la implementación:** (1) `find_latest_vps_dump` no encontraba dumps
  MariaDB (`mariadb-{uuid}` vs `{uuid}`); (2) `JSON_OBJECT` de MariaDB se rompía por backticks
  interpretados como command substitution — ahora se envía por base64 (patrón `pg_utils`).
- **Plan detallado:** `Agente/planes/completados/plan-db-compare-2026-08-28.md` (completado).
- **Documentación:** `Agente/documentacion/incidente-backups-2026-08-27.md` (método manual
  reemplazado por db-compare) y sección de comandos del README.
- **✅ db-compare-v2 + VEREDICTOS WP (09/09, commits `732add2` + `1cbefd2`):** veredicto
  `VERDE/AMARILLO/ROJO/GRIS`, baseline fijada (`--dump legacy:<sufijo>` / `vps:`), fecha
  del dump, `SinReferencia` en vez de falso "idéntica", aviso fail-closed sin baseline.
  **5/5 WP AMARILLO benigno vs legacy 13/08** (ningún ROJO/GRIS, ningún restore procede):
  guillermo 10/12, padel 25/27 (+16 spam), wandori 12/14 (+4 spam), nakomi 28/39
  (app activa, 0 pérdidas), cap 19/21 (+1 spam). Divergencias = spam post-13/08 +
  options volátiles + updates legítimos. Plan:
  `Agente/planes/completados/plan-dbcompare-v2-2026-09-09.md`.

## Incidente 2026-08-27: backups programados (dos sistemas — VPS OK, Windows roto) (DIAGNOSTICADO; studio RESTAURADO 05/09)

- **Contexto:** hay DOS sistemas de backups independientes.
- **Sistema VPS (OPERATIVO, canónico):** `/usr/local/bin/backup-server.sh` + crontab root `0 3 * * *`.
  Corre en el servidor, independiente del PC Windows. Log `/data/backups/backup.log` muestra
  `BACKUP RUN` diario con `errors=0` (16→27/08). Dumps `.sql.gz` por stack UUID con datos reales.
  El dump del 27/08 01:00 tenía todos los datos (7 proyectos) pero fue **rotado** por `daily_keep=2`;
  el único dump con datos superviviente es el **weekly `2026-08-23_0100.sql.gz`**.
- **Sistema Windows (legacy, ROTO desde ~14/08):** Task Scheduler `CoolifyManager-Backup-*` apunta al
  binario legacy `...\glorytemplate\.agent\coolify-manager-rs\target\release\coolify-manager.exe`
  que **ya no existe**. El `LastTaskResult=0` es engañoso (`.bat` con auto-ocultamiento sale con 0 sin
  ejecutar el backup real). Último backup legacy: `20260813_*`. **Era redundante, no el único.**
- **Impacto:** `studio` (nakomi.studio) con BD viva pero PGDATA reinicializado 27/08 23:35; la capa de
  negocio quedó vacía (pérdida entre 23/08 y 30/08). **✅ RESTAURADO el 05/09** desde el weekly
  `2026-08-23_0100.sql.gz` (catálogo+negocio+chat+hosting, 44 tablas; projects=6, users=13,
  services=5; 0 huérfanos FK; API/web verificadas). Detalle y método en §0 del documento del incidente.
  `kamples`, `glory-rest` y `agape` sin pérdida (VPS los respalda).
- **Detalle kamples:** falta extensión pgvector en el postgres recreado (`$libdir/vector`).
- **Documento completo:** `Agente/documentacion/incidente-backups-2026-08-27.md`
- **Pendientes (trabajo):**
  1. Confirmar cobertura del VPS para `task` (nuevo 27/08) en el próximo `BACKUP RUN`.
  2. Reinstalar pgvector en el postgres de kamples.
  3. Decidir destino del sistema legacy Windows (eliminar tareas/`.bat` o reparar apuntando al
     binario canónico); recomendado eliminarlo/documentarlo como obsoleto.
  4. [PREVENCIÓN candidata] Revisar retención de dumps (`daily_keep=2` destruyó el único dump con
     datos de studio): valorar retención mayor o promover un weekly adicional antes de rotar.
  5. **[NUEVO 09/09] Respaldar archivos WP (`wp-content`):** el VPS solo guarda BD y el legacy
     de archivos se detuvo el 13/08 → uploads/temas posteriores al 13/08 sin copia. Planificar
     backup de archivos (p. ej. extender `backup-server.sh` o tarea equivalente).
  6. **[NUEVO 09/09] Caso `guillermo` (cuenta inaccesible):** identificar la cuenta del cliente;
     reset de password solo con autorización explícita + snapshot previo. Sin rastro de hackeo.

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
