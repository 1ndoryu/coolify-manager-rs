# Plan 309A-1 — Build en tu laptop + transferencia al VPS (sin GitHub, sin build en VPS)

## Objetivo
Que ningún `deploy-service` vuelva a compilar en la VPS productiva.
La laptop compila con Docker Desktop y la imagen viaja al VPS por
`docker save → gzip → scp → docker load`. El VPS solo carga y arranca.
Sin GitHub Actions, sin registry externo, sin terceros.

## Por qué pesa GBs (respuesta a tu pregunta)
No es tu código (el binario Rust pesa decenas de MB). Es todo lo que
arrastra la imagen final: base `debian:bookworm-slim` + `libssl/curl/certs` +
frontend `dist` + capas de build. El `Dockerfile.rust` multi-stage
(`node:20-slim` + `rust:1.87-slim`) genera una imagen final típica de
300–600 MB, y el `docker save` sin comprimir la infla a ~GBs en tránsito.
Con `gzip/zstd` en el pipe se reduce mucho. Ojo: con save/scp viaja la
imagen COMPLETA en cada deploy (un registry solo enviaría las capas
cambiadas); si eso duele, el plan B es registry en el propio VPS (tarea
aparte, no este plan).

## Alcance (Rust + Kamples)
- Rust (`Dockerfile.rust`, multi-stage con `REPO_URL/BRANCH/APP_BIN/FRONTEND_DIR`).
- Kamples (`dockerfile_inline` en `kamples-stack.yaml:3-21`, build siempre en VPS hoy).
- WordPress/Minecraft NO entran (ya son pull directo, sin build).

## No alcance
- Migrar los 11 productivos. Solo desechables de prueba (`cm-test-*`);
  la migración por sitio será tarea posterior con autorización por sitio.
- Montar registry (propio o externo). Si save/scp se vuelve lento, se abre
  tarea aparte (registry en el VPS).
- Tocar la ruta GitHub Actions (`119A-4` F1–F4 sigue aparcada, intacta).
- No se rompe la ruta clásica build-in-VPS (queda para emergencias).

## Dependencias / preflight (F0)
1. Docker Desktop instalado en la laptop (HOY NO ESTÁ: `Get-Command docker`
   no devuelve nada en esta máquina). Sin esto no hay F1.
2. Espacio en `C:\tmp` y en el VPS (`docker system df`) para imagen + tar.
3. SSH operativo al VPS + sitio desechable `cm-test-309a1` (se elimina al final).
4. Decisión de tag: `cm-local/<sitio>:<sha7>` (nunca `latest`).

## Fases verificables
- **F0. Preflight.** Instalar Docker Desktop, `docker version` OK,
  `docker system df` en laptop y VPS, crear desechable. Estado máquina
  verificado 30-09: SIN Docker/Desktop, kernel WSL2 6.1.18 presente pero SIN
  distros, sesión SIN admin (la instalación la hace el usuario sí o sí),
  arch AMD64 (= VPS, sin emulación), C: 42 GB libres. 30-09 noche: Docker
  Desktop instalado (per-usuario, `AppData\Local\Programs\DockerDesktop`,
  cliente 29.8.1, backend WSL2 `docker-desktop` Running, `system df` OK,
  0 imágenes). NOTA: `docker` aún no está en el PATH de shells viejas;
  usar ruta completa o reabrir terminal. Verificación:
  `docker --version` + `ssh "docker system df"`.
- **F1. Comando `build-laptop` (Rust).** Nuevo comando: compila en la laptop
  con args leídos de la API Coolify (misma fuente que `build_env_from_coolify`;
  sin réplica exacta, el front hornea `http://127.0.0.1:3000`) +
  `--platform linux/amd64` explícito, tag `cm-local/<sitio>:<sha>` (pasa
  `validate_image_ref`: 2 segmentos con `/` bastan, `validation.rs:182`;
  el sha se resuelve del repo remoto, no local). Política de caché explícita
  (`--no-cache` como la VPS o caché documentada, sin stale silencioso).
  Transferencia SIN pipes de shell (laptop Windows): `docker save` a `C:\tmp`
  + gzip + subida con el helper existente `upload_file_streamed` (como
  `registry_login.rs:113-117`) + checksum sha256 pre/post + `docker load`
  por SSH + `docker image inspect` (tag y `Architecture`) de verificación.
  Antes de transferir: guardia fail-closed de disco (VPS ≥6 GB libres —
  tar + imagen + actual coexisten ~2x; `host_preflight.rs:44-84` exige 3 GB
  para build, insuficiente para load) + techo `C:\tmp` 7 GB (regla del área).
  Limpieza: borrar tar en ambos extremos + política keep-2 imágenes
  (`delete-site` hoy solo `rmi {uuid}-app`, no cubre `cm-local`: ampliarlo).
  `--dry-run` primero (cero transferencia). Verificación: imagen presente en
  el VPS con el tag exacto, `docker images` antes/después.
- **F2. `deploy-service` modo imagen-local.** BUG PREVIO (hallado en revisión
  30-09): el brazo `Rust if image_ref` de `compose_sync.rs:31-48` es
  INALCANZABLE por el early-return de la línea 25; `sync_compose_rust`
  jamás inyecta `image:` ni quita `build:`, así que poner `image_ref` hoy
  no cambia el compose y el swap levantaría la imagen vieja. F2 = enseñar a
  `sync_compose_rust` a sustituir `build:` por `image: cm-local/...` (o
  conmutar a `rust-image-stack.yaml`) + flag `--no-pull` fail-closed en
  `fase_build` (sin él, `docker compose pull` buscaría `cm-local/...` en
  Docker Hub público — pull-hijack). Carrier del tag por diseñar
  (`settings.json` vs flag `--image`; dos laptops no deben divergir).
  Verificación OBLIGATORIA Coolify-persiste-imagen (Coolify regenera el
  compose on-disk desde estado interno, [21C-6]): tras PATCH, GET
  `docker_compose_raw` + `grep image: cm-local` on-disk a 10 s / 60 s +
  tras restart vía API. Plan B si no persiste: `docker tag` local sin PATCH.
  Rollback: tag anterior fijado (keep-2, sin `prune -af` que lo borraría).
  Verificación: deploy de desechable con `dockerd` sin pico de CPU (nada de
  `docker compose build` en el VPS) + swap comprobado por image-ID, no solo
  por nombre.
- **F3. Kamples en laptop (mini-diseño, NO "mismo flujo").** Kamples hoy no
  pasa por `deploy-service` (`compose_sync.rs:67-72` lo rechaza; `new --image`
  lo ignora en `new_site.rs:389-394`; no existe `kamples-image-stack.yaml`).
  Faltan y se diseñan aquí: template `kamples-image-stack.yaml` (igual que el
  actual pero `image:` en vez de `dockerfile_inline`), `new --image` para
  Kamples, rama `sync_compose` Kamples-imagen, extracción del Dockerfile
  inline a fichero temporal para `build-laptop`, subida de `init-postgres.sh`
  (hoy `fases_inicio.rs:90-104` solo sube `Dockerfile.<template>`), orden del
  install del tema WP (¿antes del save sin DB o después del load con DB viva?),
  health con `app_name_hint=wordpress`, y unificar `requires_local_build`
  (`site_capabilities.rs:116` dice false, `redeploy.rs:206-207` trata Kamples
  como true). Verificación: health 200 del desechable Kamples + tema instalado.
- **F4. E2E + limpieza.** Rust y Kamples: `build-laptop` + `deploy-service`
  + health 200 + backup pre-deploy declarado (`--skip-backup` SOLO en
  desechables) + rollback con tag anterior PROBADO (no asumido). Chequeos
  anti-incidente-2026-09-20: `NRestarts` de dockerd + `docker ps` global
  antes/después + `fase_salud_colateral` (resto intacto). Cierre:
  `delete-site` de desechables (ampliado a tags `cm-local`, ver F1).
  Verificación: 200 real + dockerd sin reinicios + desechables eliminados
  sin fuga de imágenes (`docker images` limpio).
- **F5. Terminología, matriz y docs/skills.** Renombrar `build local` donde
  significa VPS (`src/commands/redeploy.rs:204,232-234`,
  `src/services/site_capabilities.rs:21-23`,
  `Agente/prevencion/prevencion-docker-restart-build-vps-2026-09-20.md:45`
  — este último es historia: solo referenciar, no reescribir; el `grep` debe
  cubrir `build local` + `requires_local_build`). Matriz canónica en ruta
  FIJA: nuevo `Agente/documentacion/deploy-imagen-local-2026-09-30.md`
  (Rust clásico=VPS | Rust imagen-local=laptop | Kamples=laptop tras F3 |
  WordPress/Minecraft=pull sin build). Actualizar SÍ: `README.md` (nuevo
  `build-laptop`, `--no-pull`, requisitos Docker Desktop + `C:\tmp`),
  `AGENTS.md` del área §9.1 (flujo imagen-local, prohibición build en VPS
  productivo, tag `cm-local`), skill `coolify-manager`
  (`C:\Users\Owner\.agents\skills\coolify-manager\SKILL.md`: añadir
  `build-laptop` + `cm-local` + no-build-en-VPS) y comentarios citados
  (`fases_nucleo.rs:23-28`, `compose_sync.rs:28-31`). NO tocar: skill
  `coolify-deployment` (genérica, delega a `--help`), prevención 2026-09-20,
  plan `119A-4` + workflow + `rust-image-stack.yaml` (aparcados), resto de
  skills y repos hermanos.
  Verificación: `grep` sin `build local` ambiguo + docs listados actualizados.

## Estado
ACTIVO 2026-09-30. Revisado por subagente (supervisor-thinker) el 30-09:
**VIABLE CON CAMBIOS** — 6 cambios obligatorios ya integrados arriba
(brazo muerto compose_sync, verificación Coolify-persiste-imagen + plan B,
mini-diseño Kamples, F1 endurecida, F4 anti-incidente, F5 con lista
docs/skills). F0/F1/F2 completadas y verificadas (check+clippy+tests).
F3 completada 30-09 (as-built abajo). Siguiente paso: F4 E2E desechables.

## F3 as-built 2026-09-30 (difiere del draft en lo marcado *)
- `config/templates/kamples-image-stack.yaml` (nuevo): espejo del actual con
  `image: {{IMAGE_REF}}` en vez de `dockerfile_inline`. Resto byte-idéntico
  (mariadb, postgres pgvector + bind `./init-postgres.sh`, 4 volúmenes).
- `new --template kamples --image <tag>`: rama Kamples+imagen en
  `generar_compose` (`new_site.rs`) + `image_ref` ya se persistía para todo
  template; `necesita_deploy_service=false` con imagen → instant_deploy=true
  (*draft pedía rama sync_compose Kamples: innecesaria, deploy-service no
  soporta Kamples y con imagen no hay nada que sincronizar/construir).
- `build-laptop --dockerfile PATH --tag TAG [--target]` (modo fichero,
  *draft pedía extraer el inline a temporal: mejorado — compila
  `config/templates/Dockerfile.kamples` como fuente canónica, sin sitio
  previo, sin vars Coolify, tag explícito). Refactor: `build_image` +
  `package_and_ship` compartidos con el modo sitio.
- Orden tema (*decidido): post-arranque en Paso 5 sobre `wordpress_data`;
  el tema NUNCA va horneado → swap de imagen jamás toca contenido.
- Drift real encontrado por el test nuevo: `Dockerfile.kamples` ≠ inline
  (comentarios + lista apt + CRLF). Fijado: `.kamples` a LF, inline
  regenerado desde el fichero (el fichero manda). Test
  `test_kamples_dockerfile_matches_inline_block` (agnóstico CRLF) +
  `test_kamples_image_stack_renders_image_without_build`.
- Riesgo abierto F4: bind relativo `./init-postgres.sh` con instant_deploy
  (¿existe en el service dir del stack nuevo?). Si falla: subir el fichero
  al service dir (como el Dockerfile en el flujo Rust) y reintentar.
- [F4 01-10 ~01:40] INTERBLOQUEO encontrado y corregido: `run_local_streaming`
  drenaba stdout hasta EOF y luego stderr; buildkit vuelca el progreso por
  stderr y el hijo bloqueaba con el buffer lleno (64 KB) mientras el lector
  esperaba un EOF imposible: build "colgado" sin salida ni CPU. Fix: dos
  lectores concurrentes (uno por stream). Lección: `docker build` manual
  siempre funcionó (streams fusionados), el manager no (pipes separados).
  También fix menor: strip del prefijo verbatim `\\?\` de canonicalize en
  modo fichero + timeout 5400 s para Kamples. Ojo sandbox: `Start-Process`
  da `ChildProcess.kill`, el timeout mata el árbol entero y `Select -First`
  corta el pipe; el build largo corre desatendido vía `Win32_Process.Create`
  (`C:\tmp\cm-f4-launch.ps1`, log `C:\tmp\cm-build-kamples-f4d.log`;
  schtasks daba carreras/fantasmas y se descartó).
- [F4 01-10 ~02:00] `npm install -g npm@latest` ROMPÍA el build: desde 2026
  npm@latest (=12.2.0) exige Node >=22 y falla con EBADENGINE sobre Node 20.
  Fix: línea eliminada de `Dockerfile.kamples` + inline de `kamples-stack.yaml`
  (basta el npm 10 de Node 20); anti-drift 21/21 OK.
- [F4 01-10 ~02:10] GloryTmpSweep purga `C:\tmp\glory-target` cada hora: el exe
  desaparece y los lanzamientos desatendidos mueren al instante. Exe F4 anclado
  en `C:\tmp\bin\coolify-manager-309a1.exe` (ese dir sobrevive) + auto-rebuild
  offline en el lanzador si falta.
- Verificación F3: `check --all-targets` + `clippy --all-targets` OK;
  `test --lib template_engine` 21 OK, `validation` 21 OK, `deploy_service`
  11 OK.
- [F4b/c 01-10 07:02-07:11] E2E Kamples `--image` VERDE a nivel contenedor:
  `new cm-test-309a1k --template kamples --image cm-local/kamples:manual001`
  → wait Kamples 150 s (fix new_site.rs) → 3/3 Up
  (wordpress `cm-local/kamples:manual001` 3c0571fe, mariadb:10.11 healthy,
  postgres pg17 healthy); WP HTTP 200; php 8.2 + pdo_pgsql + ffmpeg 7.1 +
  python 3.13 presentes; `npm install` + `npm run build` (SSG
  ExampleIsland.html) OK con Node v20.20.2/npm 10.8.2.
  HALLAZGO DB-AUTH: `WORDPRESS_DB_PASSWORD` (6JS55…24) ≠ `MYSQL_PASSWORD`
  (zryp…24) en el compose en disco pese a un único `{{DB_PASSWORD}}` en el
  render (verificado: un solo generate + un solo render + un solo POST);
  la divergencia la produce Coolify tras el create (solo mariadb; el par
  postgres `KAMPLES_PG_PASSWORD`==`POSTGRES_PASSWORD` llegó intacto).
  Reconciliado a nivel contenedor en el desechable
  (ALTER USER manager vía root) → WP-env: OK. Requiere tarea propia
  (reconciliación post-create); afecta igual al flujo classic.
  BLOQUEADOR externo: `composer install` de glorytemplate (sin
  composer.lock) aborta por advisory `PKSA-mh9b-91zm-m1gy`
  (wptrt/wpthemereview→wpcs); ni `COMPOSER_AUDIT=false` ni
  `audit.block=false` lo desbloquean.
  RIESGO restart: `restart` vía API en stack solo-imagen deja WP en Created
  e imagen `cm-local` ausente (Coolify intenta rebuild/pull).
- [F4d 01-10 07:14] Limpieza: delete-site ×2 (2.º 404 + verificado ausente),
  13 sitios intactos, settings.json limpio, 0 contenedores/volúmenes del
  uuid, `rmi cm-local/kamples:manual001` → VPS sin huella (0 cm-local,
  0 restarting). DNS .invalid sin zona: aviso esperado no bloqueante.

## Gate y Definition of Done
- Gate: `cargo fmt` + `clippy -D warnings` + `cargo test --lib` +
  `sentinel check 309A-1` PASS + E2E real en desechables (F4).
- DoD: (1) ningún deploy de prueba ejecuta `build` en el VPS;
  (2) Rust y Kamples desplegados desde imagen construida en laptop;
  (3) matriz en `Agente/documentacion/deploy-imagen-local-2026-09-30.md` +
  `build local` ambiguo eliminado + actualizados `README.md`, `AGENTS.md` §9.1
  y skill `coolify-manager` (ver F5);
  (4) roadmap actualizado, completada registrada, commit en `main`.
