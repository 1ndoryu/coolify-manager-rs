# Plan 119A-4 — Build fuera de la VPS + pull desde registry (APARCADO 2026-09-20)

> **Estado (2026-10-12):** aparcado. F1–F4 hechas en `main` (`5c75b64`, `f72493b`, `82d4f65`, `168bd05`). F5 pendiente. La alternativa sin registry ya está implementada: **309A-1** (`build-laptop`, `5aba07c`; plan `Agente/planes/completados/plan-build-laptop-2026-09-30.md`). Decisión pendiente de la usuaria: cerrar F5 con ghcr, o marcar 119A-4 OBSOLETO frente a 309A-1.

## Objetivo
Que ningún `deploy-service` vuelva a compilar Rust en la VPS productiva
(8 GB, 11 sitios). La VPS solo hace `pull` de imágenes construidas fuera.
Justificación con datos: incidente 2026-09-20
(`Agente/prevencion/prevencion-docker-restart-build-vps-2026-09-20.md`).

## No alcance
- Migrar los 11 productivos de golpe. Solo: capacidad pull + 1 site desechable
  de prueba. La migración por sitio será tarea posterior con autorización
  explícita por sitio.
- No montar registry propio (storage, backups, retención). Usar registry
  gestionado salvo decisión contraria del usuario.
- No tocar la ruta `build:` existente (sigue para emergencias); se añade ruta
  `image:` paralela.

## Decisiones del usuario (estado 2026-10-12)
1. Registry: `ghcr.io` aplicado en F1–F4.
2. Builder: GitHub Actions aplicado en F4; falta el alta por repo (aprobación de la usuaria).
3. Token: pendiente. Falta el PAT `read:packages` para el `registry-login` real en la VPS (F2).

## Fases verificables
- **F1. Settings + template. HECHA 2026-09-20** (`5c75b64`). (`imageRef`, `rust-image-stack.yaml`,
  `new --image`, validación tag fijo, `compose_sync` modo imagen; 189 tests;
  clippy 0 errores/16 warnings pre-existentes; fmt limpio en archivos tocados).
  (`ghcr.io/1ndoryu/<app>:<tag>`); `config/templates/rust-image-stack.yaml`
  = `rust-stack.yaml` con `build:`→`image: {{IMAGE_REF}}` (+ `pull_policy` si
  el compose de Coolify lo acepta; si no, pull explícito previo). Verificación:
  render del template + `config check`-like (compose config en seco si hay
  docker; si no, validación de placeholders del manager).
- **F2. `registry-login`. HECHA 2026-09-20** (`f72493b`; código + 2 tests + dry-run real
  verificado sin tocar VPS; ejecución real en VPS pendiente de PAT): token SOLO
  vía `$CM_REGISTRY_TOKEN` (nunca argv/logs), temporal remoto 0600 + login por
  stdin + borrado garantizado (`rc=$?; rm -f; exit $rc`), validación shell-safe
  de host/user. Uso real (con autorización):
  `export CM_REGISTRY_TOKEN=<PAT packages:read> && registry-login <usuario>`.
- **F3. `deploy-service` modo pull. HECHA 2026-09-20** (`82d4f65`; código local, sin tocar
  VPS): si el sitio tiene `imageRef`, `[3/6]` hace `docker compose pull` en vez
  de compilar; validación fail-fast; swap/salud/colateral sin cambios.
  Rollback operativo: fijar tag anterior + re-deploy (imagen en caché).
- **F4. Workflow Actions reutilizable. HECHA 2026-09-20** (`168bd05`; plantilla local
  `config/workflows/rust-image-ghcr.yml`, sin tocar ningún repo remoto):
  mismos build-args que la VPS (`REPO_URL/BRANCH/APP_BIN/FRONTEND_DIR`),
  login con `GITHUB_TOKEN` (sin PAT manual), tags `:sha` + `:latest-<rama>`,
  caché GHA, Dockerfile canónico traído del repo del manager. Copiada a
  `1ndoryu/task@deploy-pre-fase0` (run disparado, según roadmap). Pendiente
  lado-usuario: permisos Actions read+write si el run da 403.
- **F5. Prueba E2E con desechable. PENDIENTE.** `new --image` + `deploy-service` (pull) +
  health 200 + `delete-site`. Verificación: 200 real + resto intacto.
- **Cierre:** gate (fmt+clippy+test+check), commit, roadmap, completada.

## F6. Vía de escape si GitHub se agota (añadida 2026-09-20, sin implementar)
Lanes gratis, en orden de preferencia:
1. **PC propia como constructor de reserva** (recomendada): flag/script del
   manager (`build-local --push`) que compila con Docker Desktop y sube a
   ghcr.io. Coste 0; solo exige PC encendida + Docker instalado el día que
   se use. La VPS sigue haciendo pull igual.
2. **Segundo servidor gratis** (p. ej. Oracle Cloud free tier) como build box.
   Robusto pero otra máquina que administrar + ARM→x86 más lento.
3. **GitLab** como segunda vía gratis. Solo si hiciera falta.
Implementar la 1 cuando se agote GitHub por primera vez o lo pida el usuario.

## Riesgos
- Primer build Actions puede tardar/ajustar caché (sccache→gha-cache).
- `latest` mutable: producción siempre con tag `:sha`/versión (política).
- Token PAT: rotación manual; documentar.
