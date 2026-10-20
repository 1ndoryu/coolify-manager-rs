# Roadmap — coolify-manager-rs

> **Descripción:** Herramienta de gestión para sitios Coolify — CLI + MCP Server + GUI web + portal vps.nakomi.studio
> **Stack:** Rust/Axum (backend) + React/Vite/TypeScript (frontend GUI)
> **Repositorio:** github.com/1ndoryu/coolify-manager-rs (rama `main`)
> **Deploy:** Coolify — requiere aprobación explícita del operador antes de ejecutar

## Siguiente bloque

Nada en cola.

## Pendientes

- [ ] `11AA-1` — **Vulnerabilidad residual vigilada (`rsa 0.10.0-rc.18`, RUSTSEC-2023-0071).**
  (11-10.) El tablero (`vulnerabilidades-cache`, 11-10) reporta 1 hallazgo: `rsa 0.10.0-rc.18`
  (low, sin fix upstream), ya aceptado en 08AA-1. Decidido: sin acción local; no hay versión
  estable de `rsa` con el fix. Se reabre cuando salga `rsa` estable parcheado (desbloqueo externo).
  Su cifra previa de 5 del plan de flujo está obsoleta. **Actualizado 2026-10-12 (usuaria):
  excepción de 1 mes, vence 2026-11-12** (ver `.cargo/audit.toml`). Al vencer se revisa si hay
  `rsa` estable parcheado; si no, se renueva con fecha nueva y no se deja abierta sin fecha.

- [ ] `11AA-2` — **Residuales de vulnerabilidad con excepción hasta 2026-11-12.**
  Las alertas #33 `serde_with`, #26 `tauri` y #15 `jsonwebtoken` quedaron corregidas por el lock
  (`f6a3be1`, `serde_with` 3.21.0, `tauri` 2.11.1, `jsonwebtoken` 10.4.0) y GitHub las cerró. #23
  `rand` 0.7.3 y #13 `glib` 0.18.5 se descartaron con motivo `tolerable_risk` (decisión de la
  usuaria, 2026-10-12): `rand` llega solo vía
  `tauri-utils`→`kuchikiki`→`selectors`→`phf_generator` (unsound solo con logger propio que llame
  `rand::rng()`); `glib` solo en la GUI Linux (gtk3/webkit2gtk) y su parche exige glib ≥0.20, que
  tauri estable no trae (tauri 3 es alpha). Al vencer 2026-11-12: revisar si tauri estable trae glib
  ≥0.20 y `tauri-utils` sin `selectors` 0.24; si no, renovar con fecha nueva.

- [ ] `19AA-1` — **E11: `deploy-service` entra en rollback ciego a HTTP.**
  (Origen 268A-5, verificada en despliegue real de agape.) En un sitio NUEVO sin DNS configurado
  falla el health check HTTPS (`https://dominio/api/health` no resuelve) y entra en bucle
  rollback→rebuild (~10 min por ciclo) aunque el contenedor esté healthy y `/api/health` interno
  responda 200. Mejora: distinguir "dominio no resuelve aún" (warning, no rollback) de "app rota"
  (rollback). Idea: verificar resolución DNS del FQDN antes de tratar el fallo HTTP como fatal, o
  usar la URL interna (sslip.io / IP del contenedor) como health primario cuando el DNS del dominio
  aún no apunte.

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
