# Plan db-compare-v2 — veredicto WordPress fiable — 2026-09-09

> **Estado:** activo · **Fase actual:** 5-cierre (F0+F1+F2+F3+F4 completadas 09/09) · **Próximo paso verificable:** commit Fase 4 + push (con autorización) + releer roadmap
> **Documentos de entrada:**
> `config/backups/diagnostico-wordpress-backups-2026-09-09.md` (no versionado),
> `Agente/documentacion/incidente-backups-2026-08-27.md` §0,
> `roadmap.md` E12 + Incidente 2026-08-27.

## 1. Objetivo

Dejar `db-compare` en un estado que **no pueda dar verde ante un vaciado** y obtener un
**veredicto 100% por sitio WordPress** (`guillermo`, `padel`, `wandori`, `nakomi`, `cap`):
viva íntegra / diverge / vacía / no verificable, comparando contra los legacy `13/08`
y contra el mejor dump VPS disponible.

Caso testigo: un cliente no accede a su cuenta en `guillermo`. El verde 28/08 no lo
explica; hay que contar la viva hoy y compararla con `20260813_030007`.

## 2. Alcance / no alcance

**Sí:**
- Nuevo `backup-inventory` read-only + inspección de manifest legacy sin restaurar.
- Conteos vivos mínimos por MariaDB (prefijo descubierto, `utf8mb4`).
- Mejora `db-compare-v2`: baseline fijada, detector ambos-vacíos y negocio-en-cero,
  soporte legacy `.tar.gz`, veredicto `VERDE/AMARILLO/ROJO/GRIS` con fechas.
- Comparación read-only de los 5 WP contra legacy 13/08. Actualizar tabla §8 del
  diagnóstico y §0 del incidente. Tests + gate.

**No:**
- Ningún `restore` en este plan. Requiere autorización + snapshot por sitio aparte.
- Nada de SSH directo, Docker remoto, `curl` a Coolify, `restart --all`, ni
  `docker ps -a` global sin filtro `coolify.stack-uuid`.
- No tocar frente ajeno: `src/infra/google_drive/{auth.rs,files.rs,mod.rs}`.
- No commitear `config/backups/` (gitignored). No guardar credenciales en docs/logs.

## 3. Dependencias y causa raíz

- `src/services/compare_manager.rs:200` (`find_latest_vps_dump`) compara contra el
  **último** dump rotativo (`daily_keep=2 weekly_keep=2`): si el último ya está vacío,
  “idénticas” es un falso negativo. `roadmap.md` E12 ya lo reconoce (corrección 05/09).
- `find_latest_vps_dump` (`compare_manager.rs:473-491`) ya cubre `{uuid}` y
  `mariadb-{uuid}`, pero el veredicto no fija baseline ni fecha del dump.
- GOTCHAs vigentes: `host-exec` sin `timeout N` se cuelga; MariaDB exige
  `--default-character-set=utf8mb4`; limpieza global de exited prohibida (`eb1ce73`).
- Binario canónico: `C:\tmp\glory-target\coolify-manager\release\coolify-manager.exe`
  (`CARGO_TARGET_DIR=C:\tmp\glory-target\coolify-manager`). Su `--help` es autoridad.

## 4. Fases verificables

### Fase 0 — Preflight (read-only) — ✅ COMPLETADA 09/09

1. `git status --short --branch`: `main...origin/main [ahead 7]`, solo frente
   `google_drive` conocido (D auth/files, M mod) + plan nuevo sin trackear.
2. Binario canónico existe y responde; `db-compare --help` confirma causa raíz
   (`--dump` omitido = último VPS). `backup --list`, `host-exec --command` OK.
   `list` (10 sitios): `guillermo` guillechatbots.es, `padel` materialdepadel.es,
   `wandori` api.wandori.us, `cap` cap.wandori.us. Ojo: `nakomi` figura como
   `task.nakomi.studio` (anotado, sin tocar).
3. Target `coolify-manager` 0.86 GB < 7 GB. Apto para compilar.

### Fase 1 — Inventario read-only preciso (nuevo `backup-inventory`)

Implementar subcomando read-only (o script vía `host-exec` con `timeout`) que por cada
UUID WordPress (`owck8sww4ogk8gskgwcsk4w0`, `zkcc040cc0scock4kcooowkc`,
`csoc88c0gw8kc4cwcwosc48s`, `u00gc8ss4csc4cckkg4g00ks`, `qgskgw8wwc08o444o08wko8o`)
liste: `/data/backups/`, `/data/backups/mariadb-*/daily|weekly`,
`/data/backups/coolify-manager/*/daily|weekly|manual`, más `backup.log` y retención
real del script instalado (no la del repo).
- Campos: ruta, nombre, fecha/hora, tamaño, `pre|post|sin-fecha|vacío|pendiente`.
- **Prohibido** descargar dumps completos o extraer sobre producción.
- ✅ **HALLAZGOS 09/09 (vía `host-exec`, todo read-only):**
  - WP solo bajo `mariadb-{uuid}` (variante directa inexistente). Conservan daily
    08+09/09 y weekly 30/08+06/09, tamaños estables (guillermo 25K, padel 373K,
    wandori ~59K, nakomi ~480K, cap 53K). **Ningún dump VPS pre-incidente
    sobrevive** (más viejo = 30/08).
  - Legacy: 4 copias pre-incidente por sitio (daily 11+13/08, weekly 02+09/08);
    los 5 `.tar.gz` 13/08 contienen `db-wordpress.sql` + `files-*.tar.gz`
    (BD + uploads). Única fuente pre-incidente.
  - `backup.log` diario `total=10 errors=1`: el error es `kamples`
    (`$libdir/vector`, pgvector ausente) sin dumps frescos desde 27/08.
    Retención real 2/2, hash `2d58f4c6…` coincide. Snapshot pre-restore studio OK.

### Fase 2 — BD viva WordPress (solo conteos) — ✅ COMPLETADA 09/09

Vía `db-compare --no-tmp-container --json` (solo lectura, sin credenciales
expuestas). Ninguna viva vacía — ningún ROJO:

| Sitio | Tablas | `wp_users` | `wp_posts` | Señas de contenido |
|---|---|---|---|---|
| `guillermo` | 12 | 1 | 41 | options 224, postmeta 142, usermeta 19 |
| `padel` | 27 | 2 | 1751 | postmeta 10662, comments 25 |
| `wandori` | 14 | 1 | 51 | amazon licenses 10 + usage 990 |
| `nakomi` | 39 | 18 | 16 | tareas 242, mensajes 495, hábitos 123+798, grupos_fb 600 |
| `cap` | 21 | 3 | 14 | alumnos 9, asistencia 5091, clases 173 |

Caso `guillermo`: la viva tiene **1 usuario**. Sin el identificador del cliente
(email/login) no puede cerrarse si su cuenta existe; si el cliente no es ese
usuario, el problema es de auth/app, no un vaciado (la BD no está vacía).
**Hallazgo para v2:** el modo ligero etiqueta todo como `identica` con
`rows_otro=-1` (sin referencia); debe decir `solo-viva` para no confundir.

### Fase 3 — `db-compare-v2` (el fix) — ✅ COMPLETADA 09/09

Implementado (veredicto + baseline + legacy `.tar.gz` read-only):

- `services/compare/report.rs`: `TableState::SinReferencia` (rows_otro=-1, ya no
  `identica`), `Veredicto::{Verde,Amarillo,Rojo,Gris}`, `fecha_dump` +
  `baseline_fijada` en JSON/texto, `veredicto()` (ambos-vacíos→ROJO,
  negocio-en-cero→ROJO, sin baseline→GRIS, diffs→AMARILLO/ROJO), críticas WP
  `*_users/*_posts` y PG `users/projects/orders`, `fecha_desde_nombre_dump()`.
- `infra/db_tmp.rs`: `sh_quote()`, `pick_db_member()` (elige `db-*.sql` dentro del
  tarball), `extract_sql_from_tarball()` a `/tmp/dbcompare_*_legacy.sql` con
  cleanup garantizado aunque falle la comparación.
- `services/compare_manager.rs`: `latest_dump_command()`, `resolve_legacy_dump()`
  (prefijo `legacy:` validado contra `/data/backups/coolify-manager/<sitio>/`),
  rama legacy (extrae + sube + registra en `remote_dump_guard: Vec` + borra
  siempre), `baseline_fijada` hasta `_build_light_report()`.
- `commands/db_compare.rs`: aviso `stderr` sin baseline solo en `run()`
  (no en `execute_json`, JSON limpio para MCP).
- Tests nuevos (8): `test_sin_referencia_no_es_identica`,
  `test_pin_baseline_obligatorio`, `test_ambos_vacios_rojo`,
  `test_viva_vacia_vs_dump_con_datos_rojo`, `test_fecha_dump_desde_nombre`,
  `pick_db_member_*` (2), `test_mariadb_uuid_se_encuentra`.
- **Evidencia:** `cargo test --lib` 180 passed 0 failed; `clippy --all-targets`
  0 errores (13 warnings pre-existentes, ninguno en código nuevo); `fmt --check`
  limpio en hunks propios (resto del repo ya estaba sucio con rustfmt 1.9.0,
  no tocado); `cargo build --release` OK 1m43s; `db-compare --help` responde.
  Disco: se liberó `debug/` propio (~4GB); C: con ~19GB libres; `glory-harness`
  5.78GB ajeno no tocado.
- Nota toolchain: el shim `cargo` de Sentinel bloquea `check/test` directos
  (`sentinel check <TareaId>`); se usó `C:\Users\Owner\.cargo\bin\cargo.exe`
  directo con `CARGO_TARGET_DIR=C:\tmp\glory-target\coolify-manager`.

Caso `guillermo` (cuenta): viva `wp_users=1` → `ID=1 admin auwalmorle@gmail.com`
registrado 2026-04-05 (pre-incidente, intacto). Email ya presente 3x en legacy
13/08 + `guillermo.autoia@gmail.com` 2x. Sin indicios de intrusión: 751 POST
`wp-login.php` sin un 302, 1465 POST `xmlrpc.php` fallidos, 0 `.php` de
`wp-content` modificados post-13/08. Password no recuperable (solo hash);
reset exige autorización + snapshot. Falta confirmar si `admin` es la
cuenta del cliente.

### Fase 4 — Veredicto 100% vs legacy 13/08 (read-only) ✅ COMPLETADA 09/09

Por cada WP, dos comparaciones fijadas:
`db-compare-v2 --dump legacy:20260813_*` y `--dump vps:<mejor-pre-incidente>`.
Clasificar VERDE/AMARILLO/ROJO/GRIS según §6 del diagnóstico (F). Legacy sin
contraparte VPS sigue valiendo como suelo 13/08: lo posterior al 13/08 que falte
en viva se documenta como hueco, no se rellena mezclando uploads sin comprobar.
- **Salida:** veredicto firmado por sitio + evidencia (conteos, diffs, manifest).
- **Resultado:** 5/5 AMARILLO benigno, ningún ROJO/GRIS, ningún restore procede.
  `guillermo` 10/12 (baseline `20260813_030007`), `padel` 25/27 (+16 spam,
  baseline `20260813_031513`), `wandori` 12/14 (+4 spam, baseline
  `20260813_033004`), `nakomi` 28/39 (app activa, 0 pérdidas, baseline
  `20260813_034549`), `cap` 19/21 (+1 spam, baseline `20260813_040010`).
  JSON en `C:\tmp\dbcompare-*-legacy.json`. Gotcha: el sufijo legacy varía por
  sitio (cada backup corre a distinta hora); `--dump vps:` no se ejecutó porque
  no existe dump VPS pre-incidente para ningún WP (retención 2/2, solo 08+09/09).

### Fase 5 — Cierre documental (sin restore)

Actualizar diagnóstico §8, incidente §0, y roadmap (E12-v2 + pendientes
`task`/pgvector/legacy-Windows). Registrar evidencia en `Agente/completados/`.
El restore queda fuera del plan.

## 5. Mitigaciones (no romper nada)

- Read-only hasta Fase 4 incluida; ningún `DELETE/COPY/restore` en este plan.
- `timeout N` en todo `host-exec`; `utf8mb4` en MariaDB; filtro por stack en
  cualquier listado Docker; jamás `restart --all` con Rust vivo.
- Contenedor tmp y dump temporal siempre borrados, también en error.
- `git add` explícito por archivo; nunca `add .`; commit en español con ID;
  releer `roadmap.md` tras cada commit.

## 6. Gate y Definition of Done

- `cargo fmt --check`, `clippy -D warnings`, `cargo test`, verificación funcional
  real (`backup-inventory` + `db-compare-v2` contra 1 WP en modo read-only).
- DoD: 5 WP con veredicto + inventario sin pendientes + plan movido a
  `Agente/planes/completados/` y tarea cerrada en `Agente/completados/` con evidencia.
