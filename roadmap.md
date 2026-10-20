# Roadmap — coolify-manager-rs

> **Descripción:** Herramienta de gestión para sitios Coolify — CLI + MCP Server + GUI web + portal vps.nakomi.studio
> **Stack:** Rust/Axum (backend) + React/Vite/TypeScript (frontend GUI)
> **Repositorio:** github.com/1ndoryu/coolify-manager-rs (rama `main`)
> **Deploy:** Coolify — requiere aprobación explícita del operador antes de ejecutar

## Siguiente bloque

Nada en cola.

## Pendientes

## Bloqueos

- `119A-4` — **Build fuera de la VPS (CI + registry privado); APARCADA.** F1–F4 hechas y en `main`
  (`5c75b64`, `f72493b`, `82d4f65`, `168bd05`); la ruta clásica build-in-VPS no cambia (solo bifurca
  si hay `imageRef`). Decisión pendiente de la usuaria: cerrar F5 con ghcr, o marcar `119A-4`
  OBSOLETO frente a `309A-1` (`build-laptop`, ya implementado). Prerrequisitos lado-usuario para
  retomar: permisos Actions read+write, PAT `read:packages` para el `registry-login` real y E2E F5
  en desechable. Plan: `Agente/planes/plan-119A-4-build-externo-2026-09-20.md`.

## Planes activos

- `Agente/planes/plan-purge-docker-host-confirmacion-2026-10-20.md` — confirmación tipada en `purge-docker-host`; propuesto, espera lectura de la usuaria.
- `Agente/planes/plan-optimizacion-seguridad-vps-portable-2026-05-24.md` — optimización y seguridad portable de VPS; solo le quedan los dos E2E de reinicio real, bloqueados por ventana de la usuaria (ver «Pendiente» del plan).
- `Agente/planes/plan-119A-4-build-externo-2026-09-20.md` — `119A-4` — build fuera de la VPS (aparcado; F5 y decisión en `## Bloqueos`).
