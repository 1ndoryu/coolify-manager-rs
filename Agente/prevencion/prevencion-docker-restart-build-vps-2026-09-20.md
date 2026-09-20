# Incidente 2026-09-20 — Reinicio de dockerd en VPS productivo + caída total

## Qué pasó
- `docker.service` se reinició a las 17:19:40 CEST (Activa `ActiveEnterTimestamp`,
  `Daemon has completed initialization`, `NRestarts=4`) durante la ventana del
  build B2 de `cm-test-119a2` (Rust `--no-cache`, ~9 min a pleno CPU en VPS 8 GB
  con 11 sitios productivos + Coolify).
- El reinicio mató el build `--no-cache` (falló a los 486 s; el reintento con
  caché completó) y dejó parados TODOS los contenedores: postgres/mariadb/wordpress
  productivos `Exited (0)`, 4–6 `app-*` en `Restarting (1)` (sin DB).
- Nuestro deploy B2 abortó fail-closed en E20 post-build (postgres-test parado);
  no tocó tráfico ni volúmenes productivos.

## Causa (estado: correlación, desencadenante exacto desconocido)
- Host NO rebooteado (up 108 días). Sin OOM en `dmesg`, sin apt/upgrades, sin
  stop manual en `journalctl -u docker.service` (ventana 17:15–17:19:40).
- Los contenedores efímeros del log (`55a59`, `6579ed`) eran del propio build,
  ya eliminados — descartados como causa.
- `NRestarts=4` indica que dockerd ya se había reiniciado antes: recurrencia
  ambiental no identificada (posible inestabilidad de dockerd 27.0.3 bajo
  presión de CPU/RAM del build, sin evidencia concluyente).

## Recovery aplicado
- Fase 1 (arranques): `docker start` a BBDD (`postgres-*`, `mariadb-*`),
  espera, luego resto. 9/11 sitios volvieron (HTTP 200 verificado; cap 302
  pendiente de confirmar como su normal).
- Fase 2 (redes fantasma): `postgres-do8k`, `postgres-mo4so`, `app-mo4so`
  (objetos del 27-08) referenciaban IDs de red inexistentes → `docker start`
  imposible. Fix: `docker rm` + `docker compose up -d` acotado por servicio
  (volúmenes nombrados intactos).
- Caso especial `postgres-do8k`: volumen ANÓNIMO (`0f19bac9…`). `--force-recreate`
  lo habría sustituido vacío (= pérdida de datos). Se rescató: rename, `up`
  fresco, `cp -a` OLD→NEW con pg parado, verificación por tamaño/estructura
  (OLD 71 MB / NEW 87 MB, mismo set de bases) y `studio → 200`. `rust_db`
  con 0 tablas de usuario es su estado real (diseño de la app; `kamples` sí
  tiene 4). Volumen OLD conservado como respaldo hasta cierre.
- Backups existentes: compose-backup pre-B2 en
  `C:\Users\Owner\.coolify-manager\compose-backups\cm-test-119a2\`; backups
  rutinarios de Coolify intactos. No hizo falta restaurar nada.

## Prevención (para no repetir)
1. **119A-4 (build externo + registry)**: no volver a compilar Rust en el VPS
   productivo. El build es la carga más pesada y coincide en ventana con el
   reinicio. Prioridad máxima.
2. Si algún build local en VPS fuese inevitable: límites `cpus`/`mem` al
   contenedor de build, `CARGO_BUILD_JOBS` bajo, y aborto si `load > N` o
   RAM disponible < umbral.
3. Alerta ante reinicios de `docker.service` (`ActiveEnterTimestamp` /
   `NRestarts` cambia) y ante contenedores productivos parados: chequeo
   periódico ligero desde el manager.
4. Lección de diagnóstico: ante un E20 post-build con "bases existentes"
   vacías, comprobar primero `docker ps` global y `systemctl show docker`
   antes de asumir regeneración de compose por Coolify.
5. Cierre 20-09: 11/11 verificados HTTP (10×200 + cap 302→/cap-login/ normal);
   `cm-test-119a2` eliminado total (B3); B2-retry cancelado; 119A-4 prioritaria.
   `delete-site` idempotente ante DELETE-404 (cola async de Coolify).
