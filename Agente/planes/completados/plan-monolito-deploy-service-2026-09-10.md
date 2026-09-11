# Sub-plan — Monolito `deploy_service.rs` (`limite-lineas-nivel-3`)

**Fecha:** 2026-09-10 · **Proyecto:** `coolify-manager-rs` · **Origen:** `039A-1` Fase 4
**Hallazgo que lo motiva:** `limite-lineas-nivel-3` de Sentinel — el único error **propio** del
proyecto tras cerrar `expect-produccion-rs` (los otros errores del área son del submódulo `glory-rs`).

## 1. Medición (con el artefacto del gate, no por estimación)

| Métrica | Valor |
| --- | --- |
| Líneas totales | **2489** |
| Líneas efectivas (`contarLineasEfectivas`, Rust) | **2135** |
| Límite aplicado | **500** (`tipo=servicio`, default de Rust en `obtenerLimiteArchivo`) |
| Factor | **4,27×** → escala `nivel-3` («BASTA… Refactor obligatorio antes de seguir agregando código») |
| Distancia al nivel 4 (5× = 2500 efectivas) | **365 líneas efectivas** |
| Tamaño del siguiente módulo de `commands/` | `sync_env_helpers.rs`, **555** líneas |
| Tests | 10 marcas `#[cfg(test)]`/`#[test]`/`mod tests` **inline** en el propio archivo; el crate no tiene carpeta `tests/` |

El hallazgo **no es un falso positivo** y no admite excepción por texto: es deuda activa. A 365 líneas
efectivas de disparar el nivel 4, cualquier crecimiento lo empeora.

## 2. Objetivo

Reducir `src/commands/deploy_service.rs` por debajo del límite de 500 líneas efectivas **sin cambiar
comportamiento**: extracción mecánica a submódulos de `src/commands/deploy_service/`, con la ruta
pública (`pub async fn execute`) intacta para no tocar los llamantes.

## 3. No alcance

- Cambiar la lógica de despliegue, el orden de las operaciones ni los mensajes de error visibles.
- Refactorizar los otros 67 archivos de `commands/` «ya que estamos»: la deuda es de **este** archivo.
- Despliegues de validación contra Coolify real (fuera de alcance sin autorización explícita, ver §6).

## 4. Seams verificados (por rango de líneas y cohesión, no por tamaño)

| # | Submódulo propuesto | Contenido actual (líneas) | ≈ líneas |
| --- | --- | --- | --- |
| 1 | `compose_backup` | `backup_compose_locally`, `read_latest_compose_backup`, `simple_hash` (32-114) | ~85 |
| 2 | `postgres_inspect` | `extract_postgres_env_from_compose`, `extract_database_url_from_compose`, `validate_postgres_creds_stable` (115-294) | ~180 |
| 3 | `compose_validation` | `ComposeValidation`, `validate_compose_before_deploy` (295-450) | ~155 |
| 4 | `container_verification` | `verify_container_env_vars`, `verify_container_volumes`, `verify_postgres_data_volume` (451-602) | ~150 |
| 5 | `orchestration` | `execute()` (**603-1303**) + los helpers privados que solo usa él | **~700** |
| 6 | `compose_sync` | `sync_compose`, `rewrite_rust_service_compose`, `replace_compose_key_value`, `inject_traefik_network_label`, `inject_postgres_data_volume`, `rewrite_compose_host_rules`, `normalize_domain_host` (1304-1790) | ~490 |
| 7 | `env_building` | `BuildEnv`, `build_env_from_coolify`, `runtime_envs_from_coolify` y los predicados/escapes de env (1790-1934) | ~145 |
| 8 | `postgres_auth` | `ensure_postgres_auth_and_hostname`, `parse_env_value`, escapes SQL/sed, `base64_encode` (1935-2103) | ~170 |
| 9 | `host_preflight` | `check_server_resources`, `verify_postgres`, `verify_or_inject_traefik_network_label`, `ensure_traefik_connected`, `ensure_app_coolify_network`, `wait_for_health` (2104-2364) | ~260 |
| 10 | `rust_autoheal` | `is_rust_network_probe_failure`, `recover_rust_network_probe_failure`, `install_rust_public_autoheal`, `ensure_compose_service_image_available` y helpers de shell/systemd (2364-2612) | ~250 |

**El seam crítico es el #5:** `execute()` concentra ~700 líneas, más que el límite entero. Extraerlo
como módulo **no basta** para bajar de 500: `execute()` debe además dividirse en sus fases reales. Ese
troceado **sí** toca flujo de control y es el único punto donde la extracción deja de ser mecánica.

## 5. Fases verificables

1. **F0 — Red de seguridad primero.** Confirmar qué cubren los 10 tests inline; si `execute()` no tiene
   cobertura directa, **no** empezar por él. Salida: lista de qué fase del despliegue está cubierta y
   cuál no.
2. **F1 — Extracción mecánica de los seams 1, 2, 3, 4, 7, 8** (sin tocar flujo): mover código a
   submódulos con `mod`/`use`, visibilidad mínima necesaria. Verificación por bloque:
   `cargo build` + `cargo test --lib` + `cargo clippy --all-targets -- -D warnings` (target en `C:\tmp`
   vía `scripts/branch-db.mjs`/`run-cargo.mjs`).
3. **F2 — Extracción de los seams 6, 9, 10** (compose_sync, host_preflight, rust_autoheal). Mismo gate.
4. **F3 — Descomposición de `execute()`** en fases nombradas (preflight → env → compose → sync →
   verificación → health/rollback), cada una con su `struct` de resultado en vez de variables sueltas.
   Requiere **verificación funcional**, no solo compilación.
5. **F4 — Cierre:** `limite-lineas-nivel-3` ausente en la re-medición, `clippy -D warnings` limpio,
   suite verde, y registro de evidencia.

## 6. Riesgo y limitación real (declarados, no escondidos)

- **El archivo es el corazón del despliegue a producción.** Una extracción mecánica mal hecha (visibilidad,
  orden de `use`, captura de variables) compila y aun así cambia comportamiento. Por eso F1/F2 exigen
  `-D warnings` y tests en cada bloque, no un único gate al final.
- **La verificación funcional de F3 no está disponible hoy:** exige desplegar contra Coolify real, y este
  proyecto tiene regla explícita — toda operación remota va por `coolify-manager-rs` con **autorización
  explícita por operación+objetivo**, y `restart --all` está prohibido si hay workloads Rust. F3 no se
  declara cerrado sin esa autorización; se entrega con compilación + tests + revisión de diff, y se
  registra la limitación.
- **No se toca `deploy_service.rs` mientras haya otro bloque activo de la campaña** (`039A-1`): este
  sub-plan se ejecuta como bloque propio, con el árbol del proyecto limpio y sin WIP ajeno
  (`coolify-manager-rs` tiene WIP del usuario: borrados de `google_drive`).

## 7. Definition of Done

- [ ] `deploy_service.rs` (o `deploy_service/mod.rs`) por debajo de **500** líneas efectivas.
- [ ] Ningún submódulo nuevo por encima de 500 efectivas; `cargo clippy --all-targets -- -D warnings` sin avisos.
- [ ] `cargo test --lib` verde y sin regresión de cobertura respecto a los 10 tests previos.
- [ ] `pub async fn execute` conserva firma y ruta pública: los llamantes no cambian.
- [ ] Re-medición con el Sentinel del gate: `limite-lineas-nivel-3` ausente y sin hallazgos nuevos.
- [ ] Evidencia registrada en `Agente/completados/` + fila actualizada en la TABLA del área.
