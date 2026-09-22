# coolify-manager-rs — Roadmap

> **Descripción:** Herramienta de gestión para sitios Coolify — CLI + MCP Server + GUI web + portal vps.nakomi.studio
> **Stack:** Rust/Axum (backend) + React/Vite/TypeScript (frontend GUI)
> **Repositorio:** github.com/1ndoryu/coolify-manager-rs (rama `main`)
> **Deploy:** Coolify — requiere aprobación explícita del operador antes de ejecutar
> **Plan activo:** `Agente/planes/plan-vps-nakomi-studio-2026-05-12.md`
> **Plan activo (deuda de gate, origen `039A-1`):** `Agente/planes/completados/plan-monolito-deploy-service-2026-09-10.md` — F1/F2 completadas el 2026-09-11 (`119A-1`): `deploy_service.rs` 2702→842 líneas (2135→661 efectivas); `limite-lineas-nivel-3` eliminado, queda nivel-1 residual concentrado en `execute()` (F3, requiere verificación funcional contra Coolify real).

## Herramientas del agente
- coolify-manager-rs (este proyecto), code-sentinel, varsense (ver protocolo sección VII)

## Tareas pendientes

- **119A-5 (activa 22-09, triaje Sentinel 0.7.12):** 119 hallazgos (39E/56W/24H, 61 archivos): path-join-sin-canonicalize ×33 (E), funcion-larga-rs ×37 (W), parametros-excesivos-rs ×23 (H), sqlite-carga-N-consultas ×9 (W), god-object-rs ×10 (8W+2E), ruta-post-sin-rate-limit ×3 (E), shell-modelo-sin-allowlist ×1 (E), resto ×3. Lotes: A path-join (HECHO 22-09: A1 33->24 + shell 1->0 `42d8a76`; A2 24->0 `7bb307f`, analyze 88 con 0 path-join/shell, test PASS, clippy/fmt rojos solo por deuda ajena), B funcion-larga (splits; OJO subio 37->40 por lineas anadidas en A), C god-object (HECHO 22-09 Error 2->0: split `backup_manager` 1468 LE `dd657ee` + split `lightweight_runtime_manager` 1365 LE `fa92090`, analyze 82->80, test PASS, fmt/clippy propios limpios; + split `compare_manager` 630 LE `39dc6d8` en tipos/origen/dumps/ligero/completo, god-object 8W->7W, analyze 80->79, test PASS, 0 hallazgos propios; + split `target_bootstrap_manager` 562 LE `33bda83` en tipos/sondas/coolify/ligero, god-object 7W->6W, analyze 79->78, test PASS, 0 hallazgos propios), D1 rate-limit (HECHO 22-09 `5298cb8`: ruta-post 3->0, analyze 85), E clippy 1.95 + fmt (deuda preexistente ajena), F hints parametros (si barato). Cierre por bloque con check+test.

- **119A-2 (HECHA 20-09, ver `Agente/completados/tareas-2026-09-20.md`):** F3 + 5 helpers + B0 + `delete-site` + prueba remota B1/B2/B3 con `cm-test-119a2` (uuid `q4co88c844w0ckso88c8cc4g`, ya eliminado). B2 abortó fail-closed en E20 por reinicio de dockerd (incidente `Agente/prevencion/prevencion-docker-restart-build-vps-2026-09-20.md`): 11/11 productivos recuperados y verificados HTTP 200 (cap 302→/cap-login/ normal), sin pérdida de datos. `delete-site` ganó idempotencia DELETE-404. B2-retry en VPS productiva CANCELADO (riesgo) — la validación completa de la ruta build-in-VPS queda sustituida por 119A-4.

- **119A-3 (pendiente, origen 039A-1 triaje Sentinel 0.7.10):** splits `funcion-larga-rs` >250 líneas — `src/mcp/tools.rs` (`list_all_commands` 370 + `call_mcp_tool` 471), `src/diagnose.rs` (`diagnose` 376), `src/restore_pg_data.rs` (298), `src/services/theme.rs` (`update` 278). Los 25 `execute()` de 102–207 líneas quedan firmados en `excepciones-varsense.json` (patrón 1-comando=1-execute + tablas match). Cada split con gate + verificación funcional antes de cerrar.
  - **  - **(HECHO 20-09) diagnose + restore_pg_data splits:** diagnose.rs (533 ef) -> diagnose/{mod,secciones}.rs; restore_pg_data.rs (517) -> restore_pg_data/{mod,preparacion,aplicacion}.rs. check OK, test --lib 180/180.
  - **(HECHO 20-09) theme_manager split:** theme_manager.rs (772) -> theme_manager/{mod,fases,install,update}.rs con pub use plano (ruta externa intacta); fase_convertir_sql:102 revelado y firmado (addendum excepciones). check OK, test --lib 180/180.
  - **(20-09) limite-lineas restantes NO tocables:** deploy_service.rs:690 (bloqueado 119A-2), config/mod.rs:504 (WIP ajeno snapshot 14-09), google_drive:703 (DIFERIDO-WIP), portal.css:915 (Pablo activo VarSense GUI). Re-analisis: 58->53W.
  - **(HECHO 20-09) portal.css split + tokenizacion:** portal.css (1084) -> portal.css (base 451: tokens/font-face/nav/hero/controles) + portal-secciones.css (516: features/orbita/bloques/pricing/testimonios/faq/footer) + portal-consola.css (171: console/overlay/chart/status); 151/151 selectores conservados, cascada verificada (3 clases cross-file con orden/especificidad preservados), VistaPortal.tsx importa los 3 en orden. 12 literales -> 11 tokens --vps* nuevos (+1 reutiliza --vpsColorFondo42). Re-analisis: 53->40W (limite-lineas portal 0, hardcoded 10x 0). Residual aceptado: css-especificacion vpsOrbitPanel (sugerencia Button/ContextMenu no aplica a widget orbita; fondo ya por token).
(HECHO 20-09) mcp/tools split:** tools.rs (958 ef) -> tools/{definiciones,despacho,mod}.rs con re-export plano; helper get_opt_str (-28 repetidos); despachar_sitios <100; 6 tablas 102-138 firmadas en excepciones-varsense.json. check OK, test --lib 180/180, re-analisis 58->56W.

- **119A-4 (APARCADA 20-09 a petición del usuario; código F1–F4 en `main`, ruta clásica intacta):** build Rust fuera de la VPS (CI + registry privado, la VPS solo hace pull). Hecho y pusheado: F1 `new --image` + template `rust-image-stack.yaml` + `imageRef` (`5c75b64`), F3 `deploy-service` modo pull (`82d4f65`), F2 `registry-login` con token por env (`f72493b`), F4 plantilla `config/workflows/rust-image-ghcr.yml` (`168bd05`); workflow copiado a `1ndoryu/task@deploy-pre-fase0` (`1d62b9e`, run disparado). Tests 191/191. La ruta clásica build-in-VPS NO cambia (F3 solo bifurca si hay `imageRef`): los builds en local/VPS siguen funcionando como antes. Pendiente lado-usuario para retomar: (1) permisos Actions read+write + re-run si el run falla con 403, (2) PAT `read:packages` para `registry-login` real, (3) F5 E2E desechable (new --image + pull + health + delete-site).

## Mejoras pendientes (268A-5, verificadas en despliegue real de agape)

- **E11 rollback ciego a HTTP:** `deploy-service` en un sitio NUEVO sin DNS configurado falla el health
  check HTTPS (`https://dominio/api/health` no resuelve) y entra en bucle rollback→rebuild (~10 min
  por ciclo) aunque el contenedor esté healthy y `/api/health` interno responda 200. Mejora:
  distinguir "dominio no resuelve aún" (warning, no rollback) de "app rota" (rollback). Idea:
  verificar resolución DNS del FQDN antes de tratar el fallo HTTP como fatal, o usar la URL interna
   (sslip.io / IP del contenedor) como health primario cuando el DNS del dominio aún no apunte.

## Hallazgos B4 (20-09, test E2E ruta clásica — ver completada del día)
Ruta clásica build-in-VPS VALIDADA con binario F1–F4 (`cm-test-b4` → 200 + 11/11
intactos). **CORREGIDOS 20-09 (commit pendiente en este bloque):**
- **B4-1 (medio, CORREGIDO):** `HealthCheckConfig::rust_default()` → `/api/health`;
  `new --template rust` lo usa, resto de templates intacto (`/`). Test
  `test_rust_health_default_usa_api_health`.
- **B4-2 (medio-bajo, CORREGIDO):** E20 reintenta 6×10 s antes del veredicto
  (cubre inicialización de postgres en primer deploy); mensaje conserva lista
  de BDs para detectar drift. Lógica pura no testeable sin SSH (sin regresión:
  el veredicto final es idéntico).
- **B4-3 (bajo, CORREGIDO):** `delete-site` retira timer+service+script autoheal,
  imagen `<uuid>-app` y uploads vacíos (`rmdir`, nunca borra datos), best-effort
  con marcador `DELETE_SITE_RESTOS_OK`. Tests de unidad del comando.
- **B4-4 (bajo, CORREGIDO):** `delete_site_dns` en `delete-site` [5/6] + comando
  `delete-dns` (confirmación tipada FQDN). Solo borra si apunta a la VPS;
  conserva si apunta fuera (migración), omite duplicados. Verificado dry-run
  real: `A cm-test-b4` y `A cm-test-119a2` →   `would-delete`. **Huérfanos eliminados 20-09 con autorización explícita
  (`delete-dns --confirm` ×2, verificado `absent` posterior).**
- **B4-5 (investigar, ACLARADO):** el endpoint oficial es `POST /api/v1/deploy`
  con `uuid` en query/body (docs Coolify, citado en código); la ruta antigua
  `/services/{uuid}/deploy` no existe (404). Fix ya aplicado + test
  `deploy_usa_endpoint_oficial`.
- **Deuda nueva de toolchain (NO B4, tarea separada):** clippy 1.95 introduce
  16 errores preexistentes en ficheros no tocados (compare_manager,
  theme_manager, diagnose, db_compare, site_capabilities, template_engine,
  test [234A] en cloudflare_api) — `too_many_arguments`, `items_after_test_module`,
  etc. El código de este bloque está limpio de clippy. Registrar tarea aparte
  (fuera del alcance B4).

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
  5. **[CERRADO 10/09] Respaldar archivos WP (`wp-content`) + uploads studio:** verificado
      en producción con run completo `total=19 errors=1` (único error: kamples pgvector,
      pendiente 2). 5 `wp-content` (259–435 MB) co-ubicados en `mariadb-{uuid}`, 4 uploads
      (studio 268 MB, kamples excluido por decisión usuario), rotación 2/2 independiente
      por patrón. Incidencia en el despliegue: `set -e` + `((total++))`/`((errors++))`
      mataba el script en silencio (el legacy no tenía `set -e`); corregido a
      `total=$((total + 1))` en los 10 sitios. Prevención: todo `backup-*.sh` con `set -e`
      debe validarse con un run real mínimo, no solo `--dry-run` (el dry-run no ejecuta
      los incrementos).
  6. **[RESUELTO 10/09] Caso `guillermo`:** password del admin (`admin`,
     `auwalmorle@gmail.com`, `https://guillechatbots.es`) restablecido con autorización del
     usuario; snapshot previo `mariadb-owck8sww4ogk8gskgwcsk4w0/daily/2026-09-10_0917.sql.gz`;
     reset vía `wp_set_password` + `wp_authenticate` (`AUTH_OK`), sesiones antiguas invalidadas.
  7. **[NUEVO 10/09] Doble línea en `backup.log` (solo cosmético, NO es doble ejecución):**
     cada línea aparece 2 veces porque `log()` usa `tee -a backup.log` Y el crontab redirige
     stdout al mismo archivo (`>> backup.log 2>&1`). Verificado 10/09: syslog muestra UN solo
     disparo CRON por noche, un solo crontab root, sin timers systemd implicados, y cada línea
     del run `total=19` existe exactamente ×2. Los backups corren UNA vez. Fix propuesto
     (opcional): quitar el redirect del crontab en `install-backups.rs` o quitar el `tee`.

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
