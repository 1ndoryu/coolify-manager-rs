# Roadmap — coolify-manager-rs

> **Descripción:** Herramienta de gestión para sitios Coolify — CLI + MCP Server + GUI web + portal vps.nakomi.studio
> **Stack:** Rust/Axum (backend) + React/Vite/TypeScript (frontend GUI)
> **Repositorio:** github.com/1ndoryu/coolify-manager-rs (rama `main`)
> **Deploy:** Coolify — requiere aprobación explícita del operador antes de ejecutar

## Siguiente bloque

Nada en cola.

## Pendientes

- [ ] `19AA-2` — **E2E de `maintain-host --wait` (reinicio real) y cierre del plan de optimización.**
  Plan `Agente/planes/plan-optimizacion-seguridad-vps-portable-2026-05-24.md`. Pendiente: E2E de
  `maintain-host --wait` (reinicio real; requiere autorización explícita por operación y ventana de
  la usuaria), E2E tras reinicio real de los post-checks (1.2) y test unitario de
  `Settings::sample_sites_for_target`.

- [ ] `19AA-3` — **Pendientes del incidente de backups del 2026-08-27 que quedaron sin cerrar.**
  Salieron del bloque del incidente al migrar el roadmap; solo el punto 5 estaba marcado como
  cerrado. Re-verificar cada uno y cerrarlo o reconvertirlo en tarea propia:
  - Confirmar la cobertura del VPS para `task` (sitio nuevo del 27/08) en el próximo `BACKUP RUN`.
  - Reinstalar pgvector en el postgres de kamples (es el único error del run `total=19 errors=1`).
  - Decidir el destino del sistema legacy Windows (eliminar las tareas/`.bat` o repararlas apuntando
    al binario canónico); lo recomendado es eliminarlo o documentarlo como obsoleto.
  - Revisar la retención de dumps: `daily_keep=2` destruyó el único dump con datos de studio.
    Valorar una retención mayor o promover un `weekly` adicional antes de rotar.

- [ ] `19AA-4` — **`docker ps -a` global en tres sitios.** Revisar `docker_host_cleanup_manager.rs` y
  `target_bootstrap_manager.rs` (además del sitio ya corregido) para confirmar que su alcance global
  es intencional —limpieza explícita de host— y no el mismo riesgo.

## Bloqueos

- `119A-4` — **Build fuera de la VPS (CI + registry privado); APARCADA.** F1–F4 hechas y en `main`
  (`5c75b64`, `f72493b`, `82d4f65`, `168bd05`); la ruta clásica build-in-VPS no cambia (solo bifurca
  si hay `imageRef`). Decisión pendiente de la usuaria: cerrar F5 con ghcr, o marcar `119A-4`
  OBSOLETO frente a `309A-1` (`build-laptop`, ya implementado). Prerrequisitos lado-usuario para
  retomar: permisos Actions read+write, PAT `read:packages` para el `registry-login` real y E2E F5
  en desechable. Plan: `Agente/planes/plan-119A-4-build-externo-2026-09-20.md`.

## Planes activos

- `Agente/planes/plan-optimizacion-seguridad-vps-portable-2026-05-24.md` — `19AA-2` — optimización y seguridad portable de VPS (E2E de `maintain-host --wait` pendiente de autorización).
- `Agente/planes/plan-119A-4-build-externo-2026-09-20.md` — `119A-4` — build fuera de la VPS (aparcado; F5 y decisión en `## Bloqueos`).
