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

- **259A-1 (HECHA 25-09, triage `cargo audit` lows):** 15→5. Patch-bumps `lettre 0.11.21→0.11.23`, `quinn-proto 0.11.13→0.11.18` (+`rand` transitivo 0.9→0.10, directo `rand 0.8` intacto), `rustls 0.23.37→0.23.45`, `rustls-webpki 0.103.9→0.103.15`, `tar 0.4.44→0.4.46` (quick-xml 0.38 resuelto vía lettre). Gate `sentinel check 259A-1` **PASS** (fmt+clippy `-D warnings`+test --lib, `.quality-reports/check/259A-1/`).
- **259A-2 (HECHA 06-10, migración russh 0.46→0.64):** plan `Agente/planes/plan-259A-2-2026-10-06.md` — cierra `RUSTSEC-2026-0154/0153`; `rsa 0.9.10` eliminado, queda `0.10.0-rc.18` sin fix upstream (residual aceptado).
- **259A-3 (HECHA 07-10, quick-xml resuelto por 06AA-6):** condición cumplida sin esperar a
  tauri: `plist 1.10.0` ya trae `quick-xml 0.41.0` (testigo `Cargo.lock:3852-3853`) y
   `cargo audit` 07-10 reporta 0 quick-xml (1 vuln restante: `rsa 0.10.0-rc.18` sin fix).
- **08AA-1 (08-10, verificación + excepción ISP):** `cargo audit` = 1 vuln
  (`rsa 0.10.0-rc.18` RUSTSEC-2023-0071, `No fixed upgrade is available!`, residual
  aceptado) + 11 warnings permitidos. Excepción aceptada: sentinel ISP informativo
  `gui/src/tipos.ts:94` (`RespuestaAuditoria`, 14 campos) — DTO plano que refleja 1:1
  el JSON del backend (`audit_vps`); justificado en comentario en código (25-09-2026).
  No refactorizar.

- **299A-1 (HECHA 07-10, guard SSH cableado):** `SshClient` sonda `test -x
  /opt/coolify-guard/ssh-guard.sh` en `connect()` (`usa_guard`); `execute()`,
  `execute_binary()` y el `cat >` de `upload_file_streamed()` envuelven por el guard
  (hosts sin guard siguen en directo); veto 126 con marcador se propaga como salida +
  `tracing::warn`. Gate `sentinel check 299A-1` PASS + 3 tests nuevos. Verificado en
  principal 07-10: ALLOW `echo` OK, DENY `rm -rf /` (exit 126, nada ejecutado,
  `DENY rm-root` en `/var/log/coolify-guard.log`).
  RESIDUAL lado-usuario: standby (`standby-vps2`) rechaza la clave
  (`Permission denied (publickey,password)` 07-10; host key cambiado → probable
  reconstrucción): reinstalar la pubkey por consola; sin eso no hay guard ni SSH allí.
  Split post-cierre 07-10 (commit separado `d2b8a22`): `ssh_guard.rs` (envoltorio+
  sonda+tests) y `ssh_transfer.rs` (5 fns I/O `impl SshClient`, campos `pub(crate)`);
  `ssh_client.rs` 703→458 líneas; gate PASS + `analizar --forzar` 0E/1W/1H (solo
  preexistentes ajenos: `espera_db.rs:48`, `tipos.ts:94); `host-exec echo` OK y veto
  `rm -rf /` exit 126 re-verificados post-split en principal.

- **299A-2 (HECHA 30-09, `set-compose` para stacks no-glory):** `set-compose --name (--compose-file | --stdin) [--dry-run]`: resolución solo por nombre, validación fail-closed (ASCII puro 268A-5, `services:` nivel 0, sin `build:`/`dockerfile:` por el reinicio dockerd 2026-09-20), PATCH `docker_compose` + verificación GET con marcador `image:`. Gate `sentinel check 299A-2` **PASS** (fmt+clippy+test --lib, 6 tests nuevos) + dry-run real contra `agape` (cero escrituras; detectó y se corrigieron 4 acentos en `glory-pulse/deploy/docker-compose.yaml`). Uso real pendiente en F4 de `299A-12` (WM).

- **229A-1 (CERRADA 23-09, varsense 0 problemas):** `varsense all` 8W+1I → **0/0/0/0 en 103 archivos**. (a) Producto `8024bfb`: 6 token-duplicate mismo-`:root` eran aliases semanticos vivos → encadenados explicitos via `var()` en `gui/src/estilos/variables.css`, 0 churn en usos. (b) Detector: varsense **2.2.4** (`d9185c0`, tag `v2.2.4`) exime `style={{...}}` solo-`--*`, agrupa duplicados por archivo+ambito+valor, exime `0`/`0px`; pin fijado en `7ea079a` (push `e03e559..7ea079a`). `sentinel analyze` 0E/0W/1H (hint `RespuestaAuditoria` aceptado), gate 119A-6 PASS.

- **119A-5 (HECHA 07-10, triaje Sentinel — cierre por verificación):** CIERRE 07-10:
  `analizar --forzar` 02:30Z → 0E/0W/0I/1H en checkout completo, 0 hallazgos en sus
  7 familias (path-join/funcion-larga/parametros-excesivos/sqlite-N/god-object/
  rate-limit/shell-allowlist); testigo: la misma pasada sí detecta el hint
  `large-interface-isp` (residual aceptado 119A-6). Lotes A–F todos HECHO según historial.
  Historial: 119 hallazgos (39E/56W/24H, 61 archivos): path-join-sin-canonicalize ×33 (E), funcion-larga-rs ×37 (W), parametros-excesivos-rs ×23 (H), sqlite-carga-N-consultas ×9 (W), god-object-rs ×10 (8W+2E), ruta-post-sin-rate-limit ×3 (E), shell-modelo-sin-allowlist ×1 (E), resto ×3. Lotes: A path-join (HECHO 22-09: A1 33->24 + shell 1->0 `42d8a76`; A2 24->0 `7bb307f`, analyze 88 con 0 path-join/shell, test PASS, clippy/fmt rojos solo por deuda ajena), B funcion-larga (splits; OJO subio 37->40 por lineas anadidas en A), C god-object (HECHO 22-09 Error 2->0: split `backup_manager` 1468 LE `dd657ee` + split `lightweight_runtime_manager` 1365 LE `fa92090`, analyze 82->80, test PASS, fmt/clippy propios limpios; + split `compare_manager` 630 LE `39dc6d8` en tipos/origen/dumps/ligero/completo, god-object 8W->7W, analyze 80->79, test PASS, 0 hallazgos propios; + split `target_bootstrap_manager` 562 LE `33bda83` en tipos/sondas/coolify/ligero, god-object 7W->6W, analyze 79->78, test PASS, 0 hallazgos propios; + split `host_optimization_manager` 563 LE `1f2ed3d` en tipos/diagnostico/aplicacion/entrada, god-object 6W->5W, analyze 78->77, test PASS, 0 hallazgos propios; + split `maintenance_window_manager` 575 LE `6e6badb` en tipos/sondas/deriva/evaluacion/programacion, god-object 5W->4W, analyze 77->76, test PASS, 0 hallazgos propios; + split `cli/dispatch/ops` 559 LE `9c32dc4` en mod/plataforma/sitios/coolify/ligero/seguridad/host (waiver file-level retirado por innecesario, next-line 245A-9 preservado en coolify.rs), god-object 4W->3W, analyze 76->75, test PASS, 0 hallazgos propios), D1 rate-limit (HECHO 22-09 `5298cb8`: ruta-post 3->0, analyze 85), E clippy 1.95 + fmt (deuda preexistente ajena), F hints parametros (si barato). Cierre por bloque con check+test. CIERRE 22-09: Lote B funcion-larga 10->0 `354c10c` (an-loteb5: 272/25, 0E/10W/0I/23H; 10W = FP documentados en 229A-1 WM); Lote C clippy-needless + ctx structs CtxFix/Ramas/VentanaBloque + fmt propio `7fdfef8` (an-lotec: 272/23, 0E/10W/0I/20H; clippy propios 0 warnings, fmt propios limpio, test 196 PASS). Restan 20H ajenos (parametros en db_compare/light_site/minecraft/optimize_host/tailscale/view_logs/template_engine/backup/compare/theme_manager + 2 execute CLI con allow preexistente + large-interface tipos.ts) + deuda ajena clippy/fmt que bloquea `sentinel check` completo.

- **119A-6 (HECHA 07-10, deuda ajena saneada — cierre por verificación):** CIERRE 07-10:
  gate PASS 22-09 + `sentinel check 119A-6` PASS 07-10 (fix `todo-prosa` en
  `espera_db.rs:49`: "Todo ocurre" → "El flujo ocurre", 1 archivo) + analyze 02:30Z
  0E/0W salvo hint `RespuestaAuditoria` (`tipos.ts:94`, re-afirmado: DTO 1:1 del JSON
  de `src/api/types.rs:125-137`/`src/api/mod.rs:194-202`; partirlo rompe contrato+GUI).
  Historial 22-09: 20H (parametros-excesivos en db_compare/light_site/minecraft/optimize_host/tailscale/view_logs/template_engine/backup_manager-servidor/compare-{diff,digest,report}/theme_manager-{mod,update} + 2 execute CLI con allow preexistente + large-interface-isp tipos.ts:88) + warnings clippy ajenos + diffs fmt ajenos. Estrategia: ctx-structs en funciones internas, respetar allows/dispatchers ajenos, fmt canonico por bloque. Cierre con analyze+test por bloque. AVANCE 22-09: params-structs Copy (`Params*`, `p: &Params` + `= *p`) en minecraft/light_site/optimize_host/tailscale/view_logs/deploy_theme/new_site + `EntradaReporte` (report.rs) + `VarsTema`/`VarsKamples` (template_engine) + `CompareOptions` por valor (db_compare) + `ParamsActualizacionTema`; execute new_site/view_logs/deploy_theme <=100 LE con helpers top-level (OJO: Sentinel 0.7.12 cuenta llaves naive — helpers anidados dentro del fn INFLAN el conteo; siempre a nivel modulo). Analyze `an-119a6k`: 0E/10W/1H, parametros-excesivos 0, funcion-larga 0 (10W = 8 sqlite-FP de 229A-1 + css + ...); test 196 PASS, check+clippy lib limpios. Commit del bloque `d4ee68d`. RESTO 22-09: allows `too_many_arguments` muertos retirados (template_engine `VarsTema` es struct — lint N/A; servidor `create_site_backup_server_side` 7 params ≤ umbral); `RespuestaAuditoria` tipos.ts:89 ACEPTADO residual-hint (DTO wire plano backend→VistaDashboard/demoCoolify, ISP N/A — partirlo rompe contrato JSON+3 ficheros GUI); clippy propio 0 en tocados (1 warning ajeno diagnose/mod.rs:144 va al sweep de deuda), test 196 PASS. GATE 22-09 `sentinel check 119A-6` **PASS** (fmt+clippy+test, reporte `.quality-reports/check/119A-6/`): deuda ajena saneada — `cargo fmt` repo-wide (14 ficheros CRLF→LF) + `.gitattributes` `*.rs text eol=lf` (fija LF en checkouts/worktrees con autocrlf=true) + 4 clippy: diagnose/mod.rs:144 empty-line-doc, dns_manager.rs:22 `use super::*` sin uso (test), cloudflare_api.rs:264 `assert_eq!(x,true)`→`assert!(x)` (test), site_capabilities.rs:151 items-after-test-mod (fn movida pre-tests), template_engine.rs:474 `contains_key`. `.quality-reports/` gitignored no commiteado. CIERRE 23-09: pin sentinel 0.7.13 (`339291a`, push `090933c..339291a`): `analyze` 0E/**0W**/1H (9 sqlite-FP + 1 css-FP eliminados por el fix del detector 229A-1; solo queda el hint ISP aceptado), `sentinel check 119A-6` **PASS** fmt/clippy/test 0/0/0.

- **119A-2 (HECHA 20-09, ver `Agente/completados/tareas-2026-09-20.md`):** F3 + 5 helpers + B0 + `delete-site` + prueba remota B1/B2/B3 con `cm-test-119a2` (uuid `q4co88c844w0ckso88c8cc4g`, ya eliminado). B2 abortó fail-closed en E20 por reinicio de dockerd (incidente `Agente/prevencion/prevencion-docker-restart-build-vps-2026-09-20.md`): 11/11 productivos recuperados y verificados HTTP 200 (cap 302→/cap-login/ normal), sin pérdida de datos. `delete-site` ganó idempotencia DELETE-404. B2-retry en VPS productiva CANCELADO (riesgo) — la validación completa de la ruta build-in-VPS queda sustituida por 119A-4.

- **119A-3 (HECHA 07-10, splits funcion-larga — cierre por verificación):** CIERRE 07-10:
  analyze 02:30Z con 0 `funcion-larga-rs` y 0 `limite-lineas` en checkout completo;
  sub-splits (diagnose/restore_pg/theme/tools/portal.css) todos HECHO según historial;
  los restos citados (deploy_service:690, config:504, google_drive:703, portal.css) ya
  no generan hallazgo. Origen: triaje Sentinel 0.7.10, splits >250 líneas — `src/mcp/tools.rs` (`list_all_commands` 370 + `call_mcp_tool` 471), `src/diagnose.rs` (`diagnose` 376), `src/restore_pg_data.rs` (298), `src/services/theme.rs` (`update` 278). Los 25 `execute()` de 102–207 líneas quedan firmados en `excepciones-varsense.json` (patrón 1-comando=1-execute + tablas match). Cada split con gate + verificación funcional antes de cerrar.
  - **  - **(HECHO 20-09) diagnose + restore_pg_data splits:** diagnose.rs (533 ef) -> diagnose/{mod,secciones}.rs; restore_pg_data.rs (517) -> restore_pg_data/{mod,preparacion,aplicacion}.rs. check OK, test --lib 180/180.
  - **(HECHO 20-09) theme_manager split:** theme_manager.rs (772) -> theme_manager/{mod,fases,install,update}.rs con pub use plano (ruta externa intacta); fase_convertir_sql:102 revelado y firmado (addendum excepciones). check OK, test --lib 180/180.
  - **(20-09) limite-lineas restantes NO tocables:** deploy_service.rs:690 (bloqueado 119A-2), config/mod.rs:504 (WIP ajeno snapshot 14-09), google_drive:703 (DIFERIDO-WIP), portal.css:915 (Pablo activo VarSense GUI). Re-analisis: 58->53W.
  - **(HECHO 20-09) portal.css split + tokenizacion:** portal.css (1084) -> portal.css (base 451: tokens/font-face/nav/hero/controles) + portal-secciones.css (516: features/orbita/bloques/pricing/testimonios/faq/footer) + portal-consola.css (171: console/overlay/chart/status); 151/151 selectores conservados, cascada verificada (3 clases cross-file con orden/especificidad preservados), VistaPortal.tsx importa los 3 en orden. 12 literales -> 11 tokens --vps* nuevos (+1 reutiliza --vpsColorFondo42). Re-analisis: 53->40W (limite-lineas portal 0, hardcoded 10x 0). Residual aceptado: css-especificacion vpsOrbitPanel (sugerencia Button/ContextMenu no aplica a widget orbita; fondo ya por token).
 (HECHO 20-09) mcp/tools split:** tools.rs (958 ef) -> tools/{definiciones,despacho,mod}.rs con re-export plano; helper get_opt_str (-28 repetidos); despachar_sitios <100; 6 tablas 102-138 firmadas en excepciones-varsense.json. check OK, test --lib 180/180, re-analisis 58->56W.

- **119A-4 (APARCADA 20-09 a petición del usuario; código F1–F4 en `main`, ruta clásica intacta):** build Rust fuera de la VPS (CI + registry privado, la VPS solo hace pull). Hecho y pusheado: F1 `new --image` + template `rust-image-stack.yaml` + `imageRef` (`5c75b64`), F3 `deploy-service` modo pull (`82d4f65`), F2 `registry-login` con token por env (`f72493b`), F4 plantilla `config/workflows/rust-image-ghcr.yml` (`168bd05`); workflow copiado a `1ndoryu/task@deploy-pre-fase0` (`1d62b9e`, run disparado). Tests 191/191. La ruta clásica build-in-VPS NO cambia (F3 solo bifurca si hay `imageRef`): los builds en local/VPS siguen funcionando como antes. Pendiente lado-usuario para retomar: (1) permisos Actions read+write + re-run si el run falla con 403, (2) PAT `read:packages` para `registry-login` real, (3) F5 E2E desechable (new --image + pull + health + delete-site).

- **309A-1 (HECHA 01-10, build en laptop sin GitHub):** plan `Agente/planes/completados/plan-build-laptop-2026-09-30.md` — Rust + Kamples se compilan en la laptop (Docker Desktop) y viajan al VPS por `save→gzip→upload→load`; el VPS solo carga y arranca. Comandos: `build-laptop` (modo sitio rust + modo fichero), `new --image`, `deploy-service --image`; tag `cm-local/<sitio>:<sha12>` nunca `latest`. E2E F4 en desechable `cm-test-309a1k` (ya eliminado): 3/3 Up, WP HTTP 200 + BD OK, toolchain completo, VPS sin huella. Matriz y límites en `Agente/documentacion/deploy-imagen-local-2026-09-30.md`. `119A-4` (Actions) queda aparcada e intacta.
- **01AA-1 (HECHA 01-10, reconcile DB-auth + ensure-start + restart fail-closed):** plan `Agente/planes/completados/plan-01AA-1-2026-10-01.md` — Paso 4.5 `reconciliar_db_auth_mariadb` (ALTER manager=root tras wait, idempotente, hard-error) + `asegurar_app_levantada` (docker start si wordpress queda Created, verificado 10+ min parado) en `new --image` Wordpress|Kamples; `restart` fail-closed para `cm-local/` (single + `--all` omite). E2E en desechable `cm-test-01aa1` (ya eliminado): ensure-start + reconcile OK, 3/3 Up, HTTP 302, tema instalado/activado, 0 contenedores/volúmenes/imágenes `cm-local`, settings limpio. Divergencia confirmada sistemática (2/2 stacks imagen).
- **01AA-2 (HECHA 01-10, flag correcto):** `--no-blocking` en `composer install` (install.rs) — `--no-audit` NO existe en `install` (Composer 2.10.3) ni `COMPOSER_AUDIT=false` desbloquea; `--no-blocking` sí (phpdotenv + autoload OK en E2E). El advisory vive en require-dev, lo instalado (--no-dev) no lo contiene.
- **01AA-3 (HECHA 05-10, buildMode laptop|vps por sitio):** plan `Agente/planes/completados/plan-01AA-3-2026-10-01.md` — todo sitio nuevo nace en `laptop` (deploy compila aquí); los 13 existentes siguen en `vps` y migran uno por uno; rollback = volver a `vps`. Solo Kamples + Rust (el clásico WP usa `wordpress:latest`, no compila nada). F0–F2c+F4 con gate PASS; F2x (tail Kamples con imagen vía `POST /api/v1/deploy`) implementado con gate PASS; E2E `cm-test-01aa3` (eliminado con limpieza total): poll OK + rotación N=2 + durabilidad +10 min sin revert, health rojo solo por falta de DNS del desechable. F3 (05-10): `persistir_image_ref` tras deploy laptop verde en ramas Kamples+Rust (WARN best-effort si falla) + test roundtrip; gate PASS fmt/clippy/test 224/0. E2E de F3 diferido al próximo deploy laptop real (hoy ningún sitio en modo laptop). Comando `stop --name SITIO` añadido (para contenedores sin borrar nada, sin `--all`; rearranque por panel o `deploy-service`).

- **06AA-1 (HECHA 06-10, swap laptop verificado en producción):** `set-compose` quirúrgico verificado (build→`image: cm-local/kamples:d2993e7caf42`, +`SERVICE_FQDN_APP:`, 5 servicios, ASCII, GET-verificado ×2) + `redeploy --skip-backup` (PID 6900, build VPS ~19 min) → `Deploy exitoso`, `health http_ok=true app_ok=true`, `app-... Up (healthy)` sobre imagen NUEVA `mo4so4440c488g8woow4cow0-app:e99da147` (build fresco rama `kamples`, NO la laptop `6be6939`): el worker regenera on-disk con `build:` y el swap usa `mo4so...-app`; `cm-local:d2993e7caf42` queda cargada sin correr. Gap: sin comando CLI para `POST /api/v1/deploy` (vía durable que neutraliza el atributo `image`); `sync_compose_image` con template dropearía `redis`/`rust-app`. `settings.json` sigue `vps/null` a propósito (persistir mentiría). Divergencia conocida sin impacto: API raw=quirúrgica+cm-local (PATCHs 22:07/22:09 verificados; sync reescribió keys encima) vs on-disk=clásica; próximo sync encuentra todas las keys. Cierre 06-10 con autorización explícita: `set-compose` quirúrgico (8756 B, ASCII, marker `cm-local/kamples:d2993e7caf42`, +`SERVICE_FQDN_APP`, GET-verificado) + `official-deploy` real (`POST /api/v1/deploy` → `Service kamples started`, poll 1/8 Up sobre `cm-local`, health gate inicial 503 transitorio por arranque a los 3 s) → verificado `inspect Image=sha256:6be6939` + `health http_ok=true app_ok=true` + `settings.json` con `buildMode laptop`/`imageRef` persistido. Deuda diagnosticada 06-10 (causa raíz, solo lectura): la DB `kamples` prod tiene **0 tablas de usuario** (`pg_tables` 68 filas solo sistema; el método sí lista — testigo `pg_catalog`) y el binario **no auto-migra** (arranque sin paso migrate: sirve en 23:29:18.66 y primer error DB a los 0.06 s; `/app` sin dir `migrations/`, único env DB=`DATABASE_URL`). Relaciones faltantes observadas en logs: `ia_queue`, `cola_procesamiento_ia`, `suscripciones`, `algoritmo_estado`, `canciones`, `scraping_log`, `mv_trending_samples`, `reproducciones`. Pre-existente al swap (misma DB + misma rama bajo `e99da147`; el health HTTP pasa porque no toca DB). Fix propuesto (repo `glory-rs-template@kamples`, sin checkout local): 1) extraer DDL canónico del commit exacto del binario (laptop 06-10 vs rama VPS pueden diferir); 2) auto-migrate al arranque o job `migrate` pre-deploy + desplegar por vía normal; 3) verificar con `SELECT` + logs sin `does not exist` + health. Alternativa con auth fresca: aplicar DDL vía `host-exec` con efecto. Extra (pre-existente, no nuevo): `REDIS_URL` sin definir, VAPID/FCM/SMTP/Stripe desactivados por config.
- **06AA-2 (HECHA 06-10, backup tolerante pgvector — fix (2) backup lógico tolerante):** helper `es_error_pgvector_faltante` (`src/services/database_manager.rs`, solo firma `could not access file`+`vector` sobre `CoolifyError::Docker`); `fase_seguridad_backup` (`deploy-service`) y `backup_pre_redeploy` (`redeploy`) avisan (`ADVERTENCIA [06AA-2]`, `tracing::warn`) y CONTINÚAN sin backup DB; cualquier otro fallo sigue abortando; `--skip-backup` intacto. Gate `sentinel check 06AA-2` PASS (fmt/clippy/test 0/0/0, 5 tests nuevos, 231 passed). Diagnóstico original: pre-deploy falla `pg_dump: could not access file "$libdir/vector"` (`postgres:16-alpine` sin extensión vs DB con índices vector); solo avanza con `--skip-backup` (sin rollback DB). Causa acotada 06-10: el backup corre `pg_dump` DENTRO del contenedor postgres (`docker_exec` en `export_postgres_database`, `src/services/database_manager.rs:193-223`, cmd `:205`, error `:216`; variante server-side `:298-334`): si la imagen carece del lib `vector`, cualquier DB con índices vector rompe el dump. Fixes por recomendación: (1) `pg_dump` desde imagen con pgvector (`pgvector/pgvector:pg16`) contra la DB por red docker; (2) backup lógico tolerante que no aborte el deploy; (3) imagen postgres del stack con pgvector. Verificación real exige deploy (pendiente de autorización).
- **06AA-3 (HECHA 06-10, limpieza gate errores + execute):** 4 errores → 0 (`expect` mod.rs:90/:134 → `CoolifyError::Validation` tipado; `path-join` :55 vía `work_dir()` seguro con `join_segmento_seguro`, :552 justificación `canonicalize:` stem/tag interno) + `execute` partida en `resolver_override_laptop()` + `cola_kamples_imagen()` (sin cambios de comportamiento) + `todo-prosa` eliminado en la extracción. Gate `sentinel check 06AA-3` PASS (fmt/clippy/test 0/0/0). Quedan en 06AA-4 los 2 warnings de `build_laptop.rs` (`limite-lineas`/`god-object-rs` 651 ef).
- **06AA-4 (HECHA 06-10, splits grandes del gate):** `build_laptop.rs` (870) → `build_laptop/` (`mod` orquesta + `preflight` daemon/disco + `rotacion` tags cm-local + `pipeline` build/save-gzip/subida/load; tests repartidos por módulo); `compose_sync.rs` (787) → `compose_sync/` (`mod` sync por ramas + tests kamples + `reescritura` keys/labels/Host + `volumenes` pg_data 3 pasadas); `new_site.rs` (798) → `new_site/` (`mod` execute + `compose_render` Paso 1 + `config_persistencia` Paso 3 + `espera_db` Pasos 4/4.5 + tests + `tema` Paso 5). `funcion-larga-rs` ×4 resueltos (`fases_nucleo` `resolver_imagen_precompilada`, `dispatch/deploy` 3 dispatch_* con let-else, `restart_site` `forzar_bind_mount_rust`, `new_site` `esperar_y_reconciliar_db`). Hint `tipos.ts:94` verificado: `RespuestaAuditoria` es DTO 1:1 del JSON Rust con justificación aceptada (sin cambio). Superficie pública intacta en los 3 splits. Gate `sentinel check 06AA-4` PASS (fmt/clippy/test 0/0/0, 20 archivos) + `sentinel analyze` 0/0/0/0 en los 15 tocados.
- **06AA-5 (HECHA 07-10, comando `official-deploy` — vía durable del swap laptop):** la infra ya existe (`CoolifyApiClient::deploy_stack(uuid)` + `DEPLOY_PATH=/api/v1/deploy` + test `deploy_usa_endpoint_oficial` en `src/infra/coolify_api.rs:237-257,364-369`); falta solo superficie CLI. Alcance: fichero NUEVO `src/commands/official_deploy.rs` (`--name` → uuid de settings → `deploy_stack` → poll `8×30s` `Names+Image+" Up "` → health), cableado en `src/cli/mod.rs` + `src/cli/dispatch.rs` + `src/cli/dispatch/deploy.rs` + `src/commands/mod.rs` (grupo lifecycle `dispatch/deploy.rs:53-61` + brazo en `dispatch_deploy_lifecycle` `:192-279`, patrón `SetCompose` `:205-221` — verificado contra el refactor 06AA-4 en curso, módulo compatible sin cambios); NO tocar `deploy_service/` (frente 06AA-3/06AA-4 activo). DoD: `sentinel check 06AA-5` PASS + `--help` OK; la prueba real en producción (kamples, swap a `cm-local:d2993e7caf42` ya cargada `6be6939`) requiere autorización explícita por ser escritura remota. Cierra
06AA-1 al verificar `inspect {{.Image}}`=laptop + `buildMode laptop` + `imageRef` persistido. VERIFICACIÓN E2E 07-10 (solo lectura, sin escritura nueva: el swap se ejecutó en 06AA-1 con autorización): `docker ps` → `app-mo4so...` `cm-local/kamples:d2993e7caf42` Up healthy + `inspect Image=sha256:6be6939` `RestartCount=0`; `settings.json` kamples `buildMode laptop` + `imageRef` persistido; `https://samples.nakomi.studio/api/health` → `{"status":"ok"}`. DoD cumplido, sin autorización nueva consumida. Estado 06-10: módulo `src/commands/official_deploy.rs` creado y cableado en 4 puntos aditivos; HECHA: `sentinel check 06AA-5` PASS (fmt/clippy/test 0/0/0) + build 60 s + `--help` lista el comando + `--dry-run kamples` OK (uuid `mo4so...` + dominio, sin tocar red).
- **06AA-6 (HECHA 06-10, triaje vulnerabilidades — bloque fixable cerrado):** `cargo audit` 06-10: 5 vulns (quick-xml ×2 high `RUSTSEC-2026-0194/0195`, russh high `RUSTSEC-2026-0154`, russh-cryptovec high `RUSTSEC-2026-0153`, rsa medium `RUSTSEC-2023-0071` sin fix upstream) + 12 warnings (unmaintained/unsound/yanked transitivos, informativos). Fix aplicado: `cargo update -p plist` → `plist 1.8.0→1.10.0` + `quick-xml 0.38.4→0.41.0` (vía tauri→plist; solo 2 paquetes, resto intacto); re-audit confirma 0 quick-xml; gate `sentinel check 06AA-6` PASS (fmt/clippy/test 0/0/0, 231 passed). Resto ruteado, NO de este bloque: russh/russh-cryptovec + verificación rsa post-subida → `259A-2` (requiere migración API breaking 0.46→≥0.60). Nota: dependabot reporta 24 (conteo GitHub con transitivos/JS) vs 5 reales de `cargo audit`.

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
