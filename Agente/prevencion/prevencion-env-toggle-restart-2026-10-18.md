# Incidente 2026-10-18 — `env-toggle --restart` tumbó `mn-inmobiliaria.com`

## Qué pasó

- 23:33:45 CEST: `coolify-manager env-toggle --name inmobiliaria --key OPENCODE_GO_API_KEY --value … --restart`.
  El comando imprime `✓ OPENCODE_GO_API_KEY creado` y `✓ Servicio reiniciado`.
- Inmediatamente después, `https://mn-inmobiliaria.com/` y `/api/auth/login` devuelven **503** y
  **no queda ningún contenedor** del stack (`docker ps -a | grep as0scgwg` = 0), ni el `app` ni el `postgres`.
- Los volúmenes `as0scgwg44wkkkccgwcwg8w0_app-data` y `_pg-data` y la red sobreviven: **datos intactos**.
- Duración de la caída: hasta que se rehace el deploy (ver «Recuperación»).

## Causa

`env_toggle --restart` llamaba a **la API cruda de Coolify**
(`CoolifyApiClient::restart_service` → `POST /api/v1/services/{uuid}/restart`) y **se saltaba los dos
guardas que `restart_site` sí aplica** desde los incidentes de mayo y septiembre:

1. `template: rust` → el restart por API para el servicio y no puede volver a levantarlo sin imagen.
2. `imageRef` con prefijo `cm-local/` → imagen construida fuera (build-laptop / VPS 89) y cargada por
   SSH; **no está en ningún registry**, así que Coolify no puede recuperarla.

`inmobiliaria` cumple **las dos** condiciones a la vez (`template: "rust"`,
`imageRef: "cm-local/inmobiliaria:254bfe52e429"`), así que era el peor caso posible.

### Evidencia

- Compose en el host (`/data/coolify/services/as0scgwg44wkkkccgwcwg8w0/docker-compose.yml`, reescrito
  por Coolify a las 23:34): `image: 'cm-local/inmobiliaria:254bfe52e429'`.
- `docker images` en el 66 **no tiene ese tag** (solo el `9f802e0d5cfb` construido después): Coolify
  hizo `down` y el `up` no tenía de dónde sacar la imagen.
- Log de Coolify (UTC 21:33–21:34 = CEST 23:33–23:34):
  - `21:33:45 App\Actions\Service\RestartService RUNNING`
  - `21:34:18 App\Actions\Server\CleanupDocker RUNNING` (limpieza de imágenes en paralelo)
  - `21:34:20 RestartService 35s DONE` ← **la API responde OK aunque el servicio no vuelva**
  - `21:34:24 App\Jobs\CoolifyTask RUNNING` → `21:34:28 FAIL` ← el `docker compose up` que falla
  - `ServiceStatusChanged` x3.
- No hay fila en `application_deployment_queues`: el restart de un *service* no pasa por la cola de
  despliegues, por eso no hay log de deploy que consultar. Los logs del contenedor `coolify` son la
  única traza.
- El mensaje `✓ Servicio reiniciado` es **falso**: la API devuelve 202 al aceptar la orden, y el
  reinicio real corre después (35 s) y puede fallar.

### Por qué no lo cazó nadie antes

- El guarda existía, pero **solo dentro de `restart_site`**. `env_toggle` era un segundo camino al
  mismo endpoint de Coolify y no lo compartía. La regla estaba escrita en un sitio y aplicada en otro.
- El propio código ya avisaba del patrón en `src/api/mod.rs:365` («Gotcha: restart_site aplica
  correcciones de bind mount para Rust; no llamar la API cruda aqui») — el aviso no llegó a
  `env_toggle`.

## Arreglo (310A-1)

- Nuevo `src/services/restart_policy.rs`: **un solo** sitio decide si un sitio sobrevive a un restart
  por la API (`motivo_no_reiniciable_api`), con la regla de Rust y la de `cm-local/` juntas y con tests.
- `restart_site` pasa a usar esa política (los dos guardas duplicados, eliminados; comportamiento
  idéntico).
- `env_toggle --restart` **valida antes de escribir nada**: si el sitio no es reiniciable por API,
  falla con el motivo y no deja la variable a medias. Si lo es, delega en `restart_site` en vez de
  llamar a la API cruda.
- Mensaje honesto: `orden de reinicio aceptada por Coolify (corre en segundo plano). Confirma con
  'health --name <sitio>'`, en lugar de `✓ Servicio reiniciado`.
- Efecto colateral corregido: el crate **no compilaba en Linux** (`Docker::connect_with_named_pipe` es
  `#[cfg(all(feature="pipe", windows))]`), así que la VPS 89 no podía correr `cargo test`. Se separó
  por plataforma en `src/infra/docker_api.rs`. En Windows, idéntico.

## Prevención (para no repetir)

1. **Un endpoint de Coolify, un camino.** Si otro comando necesita reiniciar, va por `restart_site`,
   nunca por `CoolifyApiClient::restart_service` directo. `restart_policy` es la única fuente de la
   regla.
2. **No dar por bueno un `Ok` de la API de Coolify.** Responde 202 al aceptar la orden; el resultado
   real llega después (y puede ser un `CoolifyTask FAIL`). Decir «orden enviada» y ofrecer la
   verificación, nunca «hecho».
3. **Antes de un `--restart` sobre un sitio con imagen local, el reinicio seguro es `deploy-service`**
   (reconstruye imagen + bind mounts). Si el sitio es Rust o `cm-local/`, `env-toggle --restart`
   ahora lo dice y se para.
4. **Comprobar el estado tras cualquier operación de ciclo de vida**: `health --name <sitio>`; el
   silencio de la API no es un positivo.
5. Lección de diagnóstico: si un sitio productivo desaparece sin tocar nada, mirar **primero** el tag
   de imagen del compose del host contra `docker images`, y los logs del contenedor `coolify` (no hay
   cola de despliegues para los *services*).

## Recuperación

- `redeploy --name inmobiliaria` reconstruye y carga la imagen (`cm-local/inmobiliaria:9f802e0d5cfb`)
  pero **aborta en el backup pre-deploy** porque busca un contenedor `app` con imagen `glory` que ya no
  existe. Con el stack caído hay que pasar `--skip-backup`.
- El compose del host sigue pinchando el tag viejo hasta que el deploy lo reescribe; el volumen
  `_pg-data` no se toca en ningún caso.

## Referencias

- Regla de reinicio: `src/services/restart_policy.rs`, `src/commands/restart_site.rs`.
- Antecedentes del mismo tipo de fallo: `restart_site.rs` ([115A-STUDIO-AUTOHEAL], incidente 2026-05-11)
  y [01AA-1] (imagen `cm-local` perdida en restart, F4 309A-1).
- Incidente vecino (dockerd, no este): `prevencion-docker-restart-build-vps-2026-09-20.md`.
