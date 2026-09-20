# Plan 119A-4 — Build fuera de la VPS + pull desde registry (ACTIVO 2026-09-20)

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

## Decisiones pendientes del usuario (bloquean implementación)
1. Registry: propuesta `ghcr.io` (privado, coste 0, ya están en GitHub).
2. Builder: propuesta GitHub Actions por repo de app (workflow reutilizable
   que el manager genera; el usuario aprueba su alta por repo).
3. Token: PAT del usuario con `packages:write/read` (él lo crea y rota);
   el manager solo lo usa una vez para `docker login` en la VPS
   (queda en `/root/.docker/config.json`, nunca en settings/logs).

## Fases verificables
- **F1. Settings + template. HECHA 2026-09-20** (`imageRef`, `rust-image-stack.yaml`,
  `new --image`, validación tag fijo, `compose_sync` modo imagen; 189 tests;
  clippy 0 errores/16 warnings pre-existentes; fmt limpio en archivos tocados).
  (`ghcr.io/1ndoryu/<app>:<tag>`); `config/templates/rust-image-stack.yaml`
  = `rust-stack.yaml` con `build:`→`image: {{IMAGE_REF}}` (+ `pull_policy` si
  el compose de Coolify lo acepta; si no, pull explícito previo). Verificación:
  render del template + `config check`-like (compose config en seco si hay
  docker; si no, validación de placeholders del manager).
- **F2. `registry-login`.** Comando que hace `docker login <registry>` en el
  host vía host-exec con token por stdin/env (nunca en argv ni logs).
  Verificación: `docker pull` de prueba (imagen pública pequeña) + credenciales
  presentes sin exponer secreto.
- **F3. `deploy-service` modo pull.** Si el sitio tiene `imageRef`: omitir
  `fase_build`, hacer `compose pull app`, swap, health, colateral; rollback =
  re-etiquetar tag previo (instantáneo, sin rebuild). Verificación: tests
  unitarios del despacho de modo + prueba real en desechable.
- **F4. Workflow Actions reutilizable.** `Action`/`workflow` de referencia que
  compila `Dockerfile.rust` con los mismos `build-args` actuales y pushea
  `:sha` + `:latest-<rama>` a ghcr.io. Verificación: workflow en repo de prueba
  en verde e imagen visible en registry.
- **F5. Prueba E2E con desechable.** `new --image` + `deploy-service` (pull) +
  health 200 + `delete-site`. Verificación: 200 real + resto intacto.
- **Cierre:** gate (fmt+clippy+test+check), commit, roadmap, completada.

## Riesgos
- Primer build Actions puede tardar/ajustar caché (sccache→gha-cache).
- `latest` mutable: producción siempre con tag `:sha`/versión (política).
- Token PAT: rotación manual; documentar.
