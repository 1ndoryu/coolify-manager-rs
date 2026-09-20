# Plan B4 — Test E2E ruta clásica build-in-VPS con red de seguridad (2026-09-20)

## Por qué
- 119A-4 aparcada; los deploys seguirán compilando en la VPS. Hay que probar
  que la ruta clásica funciona con el binario nuevo (F1–F4), sin repetir el
  incidente dockerd del 20-09 (build `--no-cache` 9 min, load 5.9, 11 caídos).
- B2-retry estaba CANCELADO; este plan lo sustituye con red de seguridad.
  Orden explícita del usuario 20-09: "en algún momento se tiene que hacer
  deploy, no tiene que fallar, pruébalo, planifícalo bien".

## Qué se prueba
`new` clásico (sin `--image`) + `deploy-service` con build `--no-cache` +
health 200 + `delete-site`, sobre desechable `cm-test-b4.wandori.us`.
Éxito = 200 del desechable + 11/11 productivos intactos antes, durante
(muestreos) y después + cero intervenciones manuales de recovery.

## Red de seguridad (lo que evita otro accidente)
1. **Preflight con criterios de aborto** (P0): si la VPS ya va cargada, NO se
   lanza nada. Umbrales: load1 < 4.0, RAM disponible > 2 GB, dockerd
   `ActiveEnterTimestamp`/`NRestarts` anotados como baseline, `docker ps`
   sin contenedores `Restarting`, 11/11 salud baseline.
2. **Watchdog en la VPS** (P1, background nohup): cada 30 s comprueba
   load1 y timestamp de dockerd. Si load1 ≥ 8 dos chequeos seguidos → mata
   SOLO el proceso `docker compose build` (los contenedores en marcha no se
   tocan) y deja flag `/tmp/cm-watchdog.flag`. Si dockerd se reinició →
   flag para recovery inmediato. Tiempo máximo 40 min, luego sale solo.
   El manager ante build muerto aborta fail-closed (E20): no hay swap.
3. **Muestreos durante el build** (P3): cada ~3 min, `docker ps` (cero
   `Restarting`/`Exited` nuevos en productivos) + load. Si algo raro →
   abortar como en (2) y pasar a recovery del playbook conocido.
4. **Rollback armado**: E20 del manager + procedimiento 20-09 (start BBDD,
   luego resto; redes fantasma ya conocidas). Backups rutinarios intactos.
5. **Limpieza total** (P5): `delete-site` + verificar residuos + salud final
   11/11 + matar watchdog + borrar temporales.

## Fases
- **P0. Preflight** (solo lectura): load, RAM, dockerd baseline, `docker ps`
  resumen, salud 11/11. Abortar si umbrales mal.
- **P1. Watchdog**: subir script, lanzar con baseline, verificar que corre.
- **P2. `new` desechable**: `new --name cm-test-b4 --domain
  https://cm-test-b4.wandori.us --template rust --repo-url
  https://github.com/1ndoryu/task.git --app-bin glory-backend
  --frontend-dir frontend --glory-branch deploy-pre-fase0 --skip-backup`
  (nada que respaldar) + `setup-site-dns`.
- **P3. `deploy-service` clásico** (timeout 40 min) + muestreos paralelos.
- **P4. Verificación**: 200 desechable + colateral 11/11 del propio manager.
- **P5. Limpieza**: `delete-site`, residuos, salud final, stop watchdog,
  borrar `/tmp/cm-build-*`, `/tmp/cm-watchdog.*`.
- **P6. Cierre**: completada + roadmap + commit si hay cambios.

## Criterios de aborto (cualquiera → parar test, verificar 11/11, recovery si toca)
- Preflight fuera de umbrales. / Watchdog levanta flag. / Muestreo con
  contenedor productivo parado o `Restarting`. / dockerd timestamp cambia.
- Tras aborto: NO reintentar en caliente; diagnosticar primero.

## Resultado (COMPLETADO 20-09, ~21:05 CEST)
- **Veredicto: la ruta clásica build-in-VPS FUNCIONA con el binario F1–F4.**
  Deploy clásico completo OK: `Deploy exitoso! https://cm-test-b4.wandori.us/api/health
  respondiendo (status=200)`. Desechable `cm-test-b4` (uuid
  `jgkks048ocwwsogc4k48kk00`) creado, desplegado, verificado y eliminado.
- **Incidentes del test (ninguno afectó productivos):**
  1. E20 falso positivo en primer intento: postgres recién arrancado aún no
     tenía `rust_db` (16 s tras start) → abort fail-closed correcto; al
     reintentar la BD ya existía y pasó. Hallazgo: E20 necesita espera de
     readiness en primer deploy (tarea roadmap).
  2. Health `/` → 404 con app sana (`/api/health` → 200): `new --template rust`
     dejó `healthCheck.httpPath: "/"` y el manager entró en cascada rollback
     completa (restore compose, recreate, rebuild, redeploy API que dio 404
     Coolify) sobre app healthy. Tras fijar `/api/health` en settings, deploy
     PASS. Hallazgo: default de health path para Rust debe ser `/api/health`.
  3. `delete-site` fail-closed correcto (DELETE encolado, stack aún visible) +
     reintento idempotente OK (404 = borrado procesado). Residuos que hubo que
     limpiar a mano: timer `cm-autoheal-cm-test-b4` (delete-site no lo retira),
     imagen `jgkks048…-app`, `/data/uploads/cm-test-b4`, `.last`, watchdog/tmp.
     DNS `A cm-test-b4` queda (dns_manager sin borrado) — residuo conocido.
- **Seguridad validada:** watchdog `sin-flag` todo el test; dockerd
  `NRestarts=4 ActiveEnterTimestamp=17:19:40 CEST` sin cambios; colateral del
  manager 11/11 + salud final 11/11 por CLI (`health --name` ×11).
