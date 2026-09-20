# Plan 119A-2 — Descomposición `deploy_service` + test-deploy con mitigaciones

Fecha: 2026-09-20. Origen: F3 del plan monolito + triaje Sentinel 0.7.10.
Estado: FASE A COMPLETADA 20-09; Fase B autorizada 20-09 condicionada a re-revisión
de seguridad explícita. B0 IMPLEMENTADO (commit 0f108ac, test 182/182, check EXIT 0).
Dominio test fijado por usuario: cm-test-119a2.wandori.us. Borrado: el usuario exige
capacidad de borrado seguro en el manager (nuevo comando) — pendiente de implementar
antes de B1. Sin operaciones remotas hasta re-revisión cerrada + ventana acordada.

## 1. Objetivo

Cerrar `limite-lineas deploy_service.rs:690` y 5 `funcion-larga` (176/185/103/102/157)
sin cambiar comportamiento, y verificar con un test-deploy contra un sitio
desechable + limpieza total.

## 2. Inventario verificado (lectura directa 20-09)

`src/commands/deploy_service.rs` (972 líneas): `execute()` ya orquesta fases
(F1/F2/1-6/7/seed); el volumen está en `intentar_rollback` (líneas 611-810, 176 ef)
y en 4 helpers de `deploy_service/`:

| Función | Líneas | Costura de split |
|---|---|---|
| `intentar_rollback` (deploy_service.rs:611) | 176 ef | R0b restauración API + R3 recreate + R4 rebuild + R5 redeploy API (comentarios R0-R5 ya marcan las fases) |
| `inject_postgres_data_volume` (compose_sync.rs:254) | 185 ef | 3 pasadas YAML → `struct RastreoCompose::avanzar()` + `pasada_detectar_volumes` + `pasada_ultima_linea` + `reconstruir_con_inyeccion` |
| `validate_compose_before_deploy` (compose_validation.rs:89) | 103 ef | Extraer validadores por bloque (postgres/service/traefik) |
| `ensure_postgres_auth_and_hostname` (postgres_auth.rs:5) | 102 ef | Extraer alinear-rol + forzar-hostname |
| `install_rust_public_autoheal` (rust_autoheal.rs:58) | 157 ef | Extraer instalar-unidad + verificar-probe |

Sitios en `config/settings.json`: 11 productivos (5 wordpress + 6 rust), **ningún
sitio test/staging**. La Fase B necesita crear uno desechable.

## 3. Fase A — Refactor local (sin riesgo remoto, sin autorización)

1. Split `intentar_rollback` en 4 fases R0b/R3/R4/R5 con la misma firma `(&CtxDeploy, &SshClient)`.
   Re-export plano: sin cambios (fns privadas del mismo `mod`).
2. Split de los 4 helpers según costuras de §2 (tracker struct solo en compose_sync).
3. Verificación: `check --all-targets` EXIT 0 + `test --lib` 180/180 + re-análisis
   API (objetivo: `limite-lineas` deploy_service 0, 5 funcion-larga 0).
4. Commit `119A-2: ...` + roadmap. Cero cambio de comportamiento (solo movimiento
   de código; el flujo execute/R0-R5 queda idéntico).

## 4. Fase B — Test-deploy desechable (REQUIERE AUTORIZACIÓN EXPLÍCITA)

Precondición E11: un sitio NUEVO sin DNS dispara rollback ciego (health HTTPS
falla → bucle rebuild ~10 min/ciclo). Por eso el orden es estricto:

1. **[B0] Fix E11 primero** (en este mismo bloque o previo): distinguir "dominio no
   resuelve" (warning, no rollback) de "app rota" (rollback). Sin B0, la Fase B
   exige DNS propagado antes del deploy.
2. **[B1] Crear `cm-test-119a2`** vía manager: `new --template rust` con repo mínimo
   (reutilizar repo+binario de sitio existente pequeño, p.ej. `task`), `sync-env`,
   `setup-site-dns`, verificar resolución DNS **antes** de desplegar.
3. **[B2] Deploy test**: `deploy-service --name cm-test-119a2` (con backup
   pre-deploy automático incluido). Verificar health + salud colateral de los 11
   sitios (el propio flujo ya lo hace en fase_salud_colateral).
4. **[B3] Limpieza total**: destruir stack vía API Coolify, borrar registro DNS,
   verificar `docker ps -a` sin restos del stack, `/data/coolify/services/<uuid>`
   eliminado, sin bind `/data/uploads/cm-test-119a2`, stack ausente en API.
   Criterio de cierre: `diagnose` del host limpio + API sin el stack.
5. **[B4] Registrar evidencia** en `Agente/completados/` y cerrar 119A-2 + E11.

Estimación: build Rust 15-20 min + health/rollback; ventana total ~1-2 h con
polling (nohup+polling ya existe; nunca comando opaco sin heartbeat).

## 5. No-alcance

- No tocar los 11 sitios productivos (ni deploy, ni restart, ni env).
- No implementar E11 más allá de lo necesario para B2 (E11 completo es su propia tarea).
- No GUI, no varsense, no cambios de contrato CLI.

## 6. Aprobación y evidencia

- Fase A: AUTORIZADA y EJECUTADA 20-09. Splits: `intentar_rollback`→`rollback_restaurar_compose`+`rollback_intento_recreate/rebuild/api`; `inject_postgres_data_volume`→`detectar_bloque_volumes_postgres`+`ultima_linea_volumes_postgres`+`reconstruir_con_volumen_postgres`; `validate_compose_before_deploy`→`chequeo_host_backticks/imagen_no_busybox/uploads_bind/volumen_postgres/traefik_network`; `ensure_postgres_auth_and_hostname`→`resolver_credenciales_postgres`+`verificar_db_existe`+`alinear_password_y_compose`; `install_rust_public_autoheal`→`generar_script_autoheal`. Evidencia: `check --all-targets` EXIT 0, `test --lib` 180/180, re-análisis E:0 W:35 H:25 (5 funcion-larga deploy_service a 0; `limite-lineas deploy_service.rs:709ef` persiste a nivel archivo — split de archivo fuera del alcance Fase A).
- Fase B: AUTORIZADA 20-09 condicionada a re-revisión de seguridad explícita (usuario: "revisa de nuevo, que no vaya a ocurrir accidentes o caerse los demás sitios"). Pendiente definir: repo a usar, ventana horaria, B0 (fix E11) o DNS propagado.
