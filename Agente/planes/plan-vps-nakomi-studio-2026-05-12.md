# Plan vps.nakomi.studio — 2026-05-12
> Revisión del plan 2026-05-10. Incorpora diagnóstico real del código + auditoría de Nakomi Studio.

---

## ⚠ RESTRICCIÓN OPERATIVA — DEPLOY BAJO SUPERVISIÓN

**Todo deploy a producción (`vps.nakomi.studio`) requiere revisión y aprobación explícita del operador antes de ejecutarse.**

Flujo obligatorio antes de cualquier 105A-34:
1. Implementar y validar en local (GUI + backend Rust corriendo localmente).
2. El operador revisa y aprueba la sesión antes de lanzar el deploy.
3. Solo entonces se ejecuta `coolify-manager-rs deploy-service`.

Esto aplica también a los sub-deploys (build de imagen Docker, actualización de stack, cambios de configuración de Coolify). Sin aprobación explícita del operador, el agente no ejecuta ningún paso de 105A-34.

---

---

## Decisión de separación de dominios (DEFINITIVA)

| Producto | Dominio | Qué hace |
|---|---|---|
| **WordPress Hosting** | `nakomi.studio/soluciones/hosting` | Se queda donde está. No se mueve. |
| **VPS** | `vps.nakomi.studio` | Portal dedicado. Landing, compra, panel de cliente y panel admin. |

- Nakomi Studio conserva `/soluciones/vps` solo como entrada comercial/marketing con CTA que lleva a `vps.nakomi.studio`.
- Todo el flujo de compra, provisioning, panel de cliente y administración de VPS vive en el nuevo portal.
- El producto se presenta como **Nakomi VPS** — infraestructura propia, sin mención de revendedor ni de Contabo como proveedor base. Referencia de experiencia: [contabo.com](https://contabo.com) (precios, UX, nivel de control). No se copia ni menciona.

---

## Evaluación de floci (github.com/floci-io/floci)

**Resultado: no aplica a este proyecto.**

Floci es un **emulador local de AWS** (LocalStack replacement), escrito en Java, sin relación con gestión de VPS ni provisioning de infraestructura. Su valor es para equipos que usan AWS SDK (S3, SQS, Lambda, RDS, etc.) y necesitan un entorno local sin cuenta AWS.

No existe uso case en `vps.nakomi.studio` a menos que en el futuro se decida migrar almacenamiento de backups/artefactos a S3-compatible — en ese caso Floci serviría como emulador de test. Registrado como opción futura, no acción inmediata.

---

## Auditoría de Nakomi Studio (glory-rust-template)

### Lo que ya existe y es reutilizable

| Componente | Ubicación | Estado |
|---|---|---|
| Modelo de datos VPS | `src/models/vps.rs` | Completo: `VpsPlanConfig`, `VpsSubscription`, `VpsEvent` |
| Handlers VPS | `src/handlers/vps.rs` | Tiene: catálogo público, checkout Stripe, aprobación manual, provisioning |
| Contabo API | `src/services/contabo.rs` | OAuth2, listado de instancias, `ContaboInstance` con IP/CPU/RAM/disco |
| Stripe VPS | `src/services/vps_stripe.rs` | Checkout recurrente, pending_approval, reject+refund flow |
| Auth | `src/handlers/auth.rs` + argon2 + jsonwebtoken | Ya existe en Nakomi — reutilizable directamente |
| Frontend VPS | `SolucionVpsIsland.tsx` | Página `/soluciones/vps` con planes, features y ModalCompra |
| Frontend Hosting | `SolucionHostingIsland.tsx` | Página `/soluciones/hosting` — se queda aquí |
| Soluciones landing | `SolucionesIsland.tsx` | Grid con Hosting + VPS + Agentes IA — actualizar VPS para apuntar a `vps.nakomi.studio` |

### Flujo VPS actual en Nakomi

```
Cliente compra en /soluciones/vps
  → Stripe checkout (suscripción mensual)
  → Estado: pending_payment → pending_approval
  → Admin aprueba: Contabo API crea instancia
  → Estado: provisioning → active
  → Email al cliente con IP, usuario, contraseña inicial
  → Si rechaza: cancela suscripción Stripe + intento de refund
```

Este flujo ya está implementado en Nakomi. La decisión es si **migrarlo** al nuevo portal o **exponer la misma API** desde un segundo dominio con auth propio.

### Decisión de arquitectura para el MVP

**Opción A — Puerto API de Nakomi:** `vps.nakomi.studio` es un frontend React que consume la API de `nakomi.studio` (o un subdomain API separado), con auth propio del portal (JWT portal → validado por el backend de Nakomi con rol `admin`/`vps_customer`).

**Opción B — Backend separado:** `vps.nakomi.studio` tiene su propio backend Rust/Axum que replica/adapta la lógica existente.

**Decisión MVP: Opción A.** El backend de Nakomi ya tiene el dominio completo. El portal VPS es un frontend React con auth propio que llama a endpoints `/api/vps/**` de Nakomi con un header de origen autorizado. Evita duplicar toda la lógica. El `coolify-manager-rs gui-api` sigue siendo solo para la consola de operador interno.

### Lo que hay que cambiar/limpiar en Nakomi antes de migrar

1. Eliminar de textos públicos cualquier referencia a Contabo como proveedor (hay una línea en `SolucionVpsIsland.tsx`: `"sin markup artificialmente inflado sobre Contabo"`).
2. `/soluciones/vps` pasa a ser landing de marketing con CTA → `vps.nakomi.studio` en vez de formulario de compra directo (o conserva ambos como entrada alternativa durante la transición).
3. Revisar `humanize_tier_name` en `vps_stripe.rs`: `"Cloud VPS 1"` etc. — alinearlo con la marca Nakomi VPS.

---

## Estado real del código (diagnóstico 2026-05-12)

| Elemento | Estado |
|---|---|
| `App.tsx` | Va directo al dashboard sin login wall — **sin auth** |
| `gui_api.rs` | Solo `/health` + `/api/command`; CORS `allow_origin(Any)` — **sin auth endpoints** |
| `VistaLogin.tsx` | **No existe** |
| Landing pública | **No existe** |
| Argon2/bcrypt en Cargo.toml | **No existe** (sí hay `jsonwebtoken = "9"` y `rand`, `sha2`) |
| Bootstrap admin | **No existe** |
| Rate limiting | **No existe** |
| CORS cerrado | **No existe** (abierto a `Any`) |

**Completados del plan anterior:**
- 105A-28: Caché y optimización de GUI operativa ✓
- 105A-40: Documento de arquitectura técnica ✓
- 105A-43: Routing inicial (landing `/soluciones/vps` en Nakomi, no en este repo) — parcial

**Hallazgo crítico:** El plan 2026-05-10 marcaba el deploy (105A-34) antes de implementar login (105A-35/41).  
Login + auth son **bloqueadores** del deploy, no tareas posteriores.

---

## Contexto

La GUI de `coolify-manager-rs` debe poder publicarse como `vps.nakomi.studio` con login, administración segura y evolución hacia compra/gestión de VPS vinculada a Nakomi Studio.

---

## Decisiones base (sin cambios)

1. `vps.nakomi.studio` es un producto/portal separado de la web principal.
2. El primer MVP online es read-only para infraestructura; write se habilita después de auth/RBAC/auditoría.
3. GUI local y app online comparten componentes; no comparten boundary de permisos.
4. El navegador nunca debe recibir `settings.json`, tokens de Coolify, claves SSH ni paths internos.
5. Deploy de `vps.nakomi.studio` via `coolify-manager-rs` y Coolify; SSH directo solo emergencia documentada.
6. Compra directa de VPS no implementada todavía.

---

## Arquitectura objetivo (sin cambios)

- **Frontend:** React + Vite, consola operativa, login obligatorio antes de cualquier panel.
- **Backend portal:** Rust/Axum con auth, dashboard, auditoría y acciones permitidas.
- **Orquestador interno:** `coolify-manager-rs` como librería/servicio.
- **Fuentes externas:** Coolify API, Contabo API, Stripe.

---

## BLOQUEADORES PREVIO AL DEPLOY

Los siguientes ítems deben estar completos antes de cualquier despliegue online.  
Sin ellos, la app expone operaciones internas sin ninguna protección.

### BLOQUEO 1 — Auth backend (gui_api.rs)
- Agregar `argon2 = "0.5"` a `Cargo.toml`.
- Tabla/store de usuarios: en-memoria para MVP (Vec protegido por RwLock) o SQLite con `rusqlite`.
- Endpoints nuevos en el Router de Axum:
  - `POST /api/auth/login` — recibe `{email, password}`, valida hash Argon2, devuelve JWT de vida corta (15min) + refresh token (7d) en cookie `HttpOnly Secure SameSite=Lax`.
  - `POST /api/auth/logout` — invalida refresh token.
  - `GET /api/auth/me` — devuelve `{email, role}` si sesión válida.
- Middleware de auth que protege todas las rutas `/api/command/**`.
- CORS: cambiar de `allow_origin(Any)` a origen explícito (env `ALLOWED_ORIGIN`, default `http://localhost:5173`).
- Rate limit en `/api/auth/login`: máximo 5 intentos / IP / 15min (tower con state o `governor`).

### BLOQUEO 2 — Bootstrap admin
- Al arrancar `gui_api`, si no existe ningún usuario admin, leer `ADMIN_EMAIL` y `ADMIN_PASSWORD` de env y crear el usuario con hash Argon2.
- Si no se pasan esas vars y no hay admin → imprimir advertencia, **no arrancar** en modo online (o arrancar en modo local sin auth si `LOCAL_MODE=true`).
- Prohibido hardcodear credenciales en código. Prohibido loguear la password.

### BLOQUEO 3 — Login wall en frontend
- Crear `gui/src/componentes/VistaLogin.tsx`: formulario email + password, feedback de error visible, spinner durante petición.
- Crear `gui/src/hooks/useAuth.ts`: estado `{ autenticado, usuario, token }`, funciones `login(email, password)` y `logout()`, persistencia del JWT en memoria (no localStorage para tokens de auth).
- Modificar `App.tsx`: si `!autenticado` → renderizar `<VistaLogin />` en vez del layout operativo.
- El token se adjunta como `Authorization: Bearer ...` en todas las llamadas a `/api/command`.

### BLOQUEO 4 — Modo local vs online
- La GUI detecta si está corriendo contra un backend local (`localhost`) o remoto.
- En modo local (operador): se puede omitir auth con `LOCAL_MODE=true` en env del servidor, para no interrumpir el flujo de trabajo actual.
- En modo online: auth siempre obligatorio, sin bypass.

---

## Backlog ejecutable — por fases

### Fase 0 — Desbloqueadores del deploy (ANTES del 105A-34)

#### 125A-1 — Auth backend en gui_api.rs
- Añadir `argon2 = "0.5"` en `Cargo.toml`.
- Implementar store de usuarios en memoria o SQLite (MVP: `Vec<User>` en `Arc<RwLock<...>>`).
- Endpoints `POST /api/auth/login`, `POST /api/auth/logout`, `GET /api/auth/me`.
- Middleware Axum que valida JWT en `Authorization: Bearer` para rutas `/api/command`.
- CORS: `allow_origin` desde env `ALLOWED_ORIGIN`.
- Rate limit en login con `governor` o state manual.
- Tests unitarios: hash válido, hash inválido, expiración JWT.

#### 125A-2 — Bootstrap admin seguro
- Leer `ADMIN_EMAIL` + `ADMIN_PASSWORD` de env al arrancar.
- Hashear con Argon2, almacenar en store de usuarios.
- Si faltan vars y no hay admin existente → error claro al arrancar en modo online.
- `LOCAL_MODE=true` → omite auth para operador local.

#### 125A-3 — Login wall en frontend
- `VistaLogin.tsx`: form email/password, validación básica, error inline, loading state.
- `useAuth.ts`: estado de sesión, funciones login/logout, token en memoria.
- `clienteCoolify.ts`: adjuntar `Authorization: Bearer` a todas las llamadas.
- `App.tsx`: guard `if (!autenticado) return <VistaLogin />`.
- CSS en sistema de diseño existente (variables.css, no estilos ad-hoc).

#### 125A-4 — Landing mínima en `/`
- Cuando el usuario no está autenticado, `vps.nakomi.studio/` muestra pantalla compacta:
  logo + tagline + botón "Entrar" → redirige a `/login`.
- Para MVP: la propia `VistaLogin` puede ser la landing (sin página separada).
- Decisión: si se quiere ruta `/login` separada o landing en `/` con modal de login. Definir antes de implementar.

---

### Fase 1 — Rendimiento local y UX base (ya iniciada)

#### 105A-29 — Selector global de VPS en logoSidebar
- El cambio de VPS vive en la zona de marca/sidebar, no como select suelto.
- No usar `<select>` nativo. Usar `SelectorPersonalizado` de `componentes/ui/`.
- Ya existe `SelectorVps.tsx`; verificar si ya cumple o necesita ajuste.

#### 105A-31 — Agregar sitio como modal funcional
- `Agregar sitio` abre `ModalAgregarSitio` (ya existe el componente), no navega a Ajustes.
- Conectar a endpoint real cuando exista; si falta, mostrar estado explícito.

#### 105A-32 — Retirar rutaPagina
- Eliminar `rutaPagina` de las vistas; ajustar espaciado del header.

#### 105A-33 — Favicons inline para sitios
- Tabla de sitios usa favicon real del dominio como icono.
- Fallback determinista si no carga.
- No bloquear el listado por errores de favicon.

#### 105A-30 — Sentinel contra selects nativos
- Regla en Glory Sentinel: detectar `<select>` nativo en React/TSX.
- Recomendar `SelectorPersonalizado` o equivalente del sistema.
- Cubrir con fixture/test.

---

### Fase 2 — Deploy online (desbloqueada tras Fase 0)

#### 105A-34 — Despliegue online `vps.nakomi.studio`
- **Prerrequisito:** 125A-1, 125A-2, 125A-3 y 125A-4 completados.
- Desplegar frontend (dist de Vite) y backend Rust como servicio separado en Coolify.
- Dominio: `vps.nakomi.studio`. No reutilizar stack `studio`.
- Env de producción: `ADMIN_EMAIL`, `ADMIN_PASSWORD`, `ALLOWED_ORIGIN=https://vps.nakomi.studio`, `LOCAL_MODE=false`, `JWT_SECRET` (generado y almacenado en Coolify secrets).
- Deploy via `coolify-manager-rs deploy-service`. Health check tras deploy. Rollback si falla.

---

### Fase 3 — MVP online seguro

#### 105A-36 — Seguridad operativa online
- RBAC: roles `admin`, `operator`, `viewer`.
- Auditoría: tabla de eventos (actor, rol, acción, target, resultado, IP, timestamp). Sin secrets en logs.
- Confirmación fuerte para acciones destructivas desde navegador.
- Límites de payload y timeout en llamadas externas.

#### 105A-42 — API read-only de infraestructura
- DTOs seguros que exponen solo datos normalizados: nombres, estados, métricas, timestamps, IDs públicos.
- Sin paths internos, tokens ni config cruda.
- Protegidos por middleware de auth ya implementado en 125A-1.

#### 105A-44 — Auditoría y permisos write
- Tabla/stream de eventos de auditoría.
- Habilitar backup manual, logs filtrados, restart/redeploy por fases.
- Cada acción write: evento antes y después, actor y resultado registrados.

---

### Fase 4 — Producto VPS completo

#### 105A-45 — Migración y limpieza en Nakomi Studio
- **Auditoría completada** (ver sección de arriba). Resumen de acciones:
  - Eliminar referencia a Contabo en `SolucionVpsIsland.tsx` (texto "sin markup artificialmente inflado sobre Contabo").
  - Actualizar `/soluciones/vps` como landing marketing con CTA → `vps.nakomi.studio`.
  - Renombrar tiers en `vps_stripe.rs` de "Cloud VPS N" a "Nakomi VPS N" para consistencia de marca.
  - `SolucionesIsland.tsx`: card de VPS actualiza `enlace` a `https://vps.nakomi.studio`.
- WordPress Hosting (`/soluciones/hosting`) no se toca — se queda en Nakomi Studio.
- No mover flujos sin revisar compras VPS activas primero (tabla `vps_subscriptions` en producción).

#### 105A-37 — Portal VPS conectado a API de Nakomi
- El frontend de `vps.nakomi.studio` consume `/api/vps/**` de Nakomi como backend (Opción A).
- Auth del portal: JWT propio del portal → verificado por un middleware de Nakomi con `role = admin` o `role = vps_customer`.
- Panel de cliente: ver suscripción, estado de provisioning, IP, facturación (Stripe customer portal link).
- Panel admin: aprobar/rechazar suscripciones pendientes, ver instancias Contabo, ver eventos VPS.
- Integrar Stripe/Contabo/Coolify usando los servicios ya implementados en Nakomi, no reimplementar.
- Referencia de UX/producto: contabo.com — estructura de planes, página de servidores, panel de cliente. Sin mencionar al proveedor en ningún texto público.

---

## Orden de ejecución recomendado

```
[Fase 0 — Desbloqueadores del portal]
125A-1 (auth backend gui_api)
  → 125A-2 (bootstrap admin)
  → 125A-3 (login wall frontend operador)
  → 125A-4 (landing mínima portal)
     → 105A-34 (DEPLOY portal operador en vps.nakomi.studio)

[Fase 1 — UX GUI operativa, paralelo a Fase 0]
105A-29, 105A-31, 105A-32, 105A-33, 105A-30

[Fase 2 — Seguridad online, tras deploy]
105A-36, 105A-42, 105A-44

[Fase 3 — Producto VPS, conectado a Nakomi]
105A-45 (limpiar Nakomi: referencias Contabo, CTA → vps.nakomi.studio)
  → 105A-37 (portal VPS consume API Nakomi, panel cliente + admin)
```

**Nota arquitectura:** El portal de `vps.nakomi.studio` en su fase producto (105A-37) consume `/api/vps/**` del backend de Nakomi Studio directamente — no duplica la lógica de Contabo, Stripe ni provisioning que ya está implementada. El `coolify-manager-rs` con su `gui_api` es la consola interna de operador (Fase 0), no el backend del portal de cliente.

---

## Seguridad obligatoria (sin cambios del plan anterior)

- Password hash con **Argon2** (añadir crate).
- Sesión: cookie `HttpOnly Secure SameSite=Lax` o JWT de vida corta + refresh seguro.
- Rate limit por IP/cuenta en login, refresh y acciones críticas.
- CORS cerrado al dominio de producción (env `ALLOWED_ORIGIN`).
- HTTPS obligatorio en producción.
- CSP básica para scripts/assets propios.
- Nunca registrar secretos, tokens, passwords, headers completos ni logs crudos.
- El frontend recibe solo datos normalizados — nunca `settings.json` crudo.

---

## Riesgos

1. **Exponer app sin auth** — ya ocurre localmente; debe resolverse antes de cualquier dominio público.
2. **`LOCAL_MODE=true` en producción** — configuración incorrecta saltaría todo el auth. Verificar en health check de deploy.
3. **Mezclar GUI local con portal online** — el mismo `gui_api.rs` sirve los dos modos; la separación debe ser explícita vía env, no vía detección heurística.
4. **Token en localStorage** — no usar para JWT de auth. Solo memoria de JS o cookie HttpOnly.
5. **Duplicar lógica de Nakomi Studio** en vez de reutilizar servicios.

---

## Criterios de listo

- Login obligatorio antes de cualquier dato operativo (sin bypass accidental).
- Ningún endpoint online expone secretos, config cruda u operaciones sin auth.
- Bootstrap admin sin hardcodeo; credentials solo en env.
- MVP online arranca read-only; write habilitado solo con RBAC y auditoría.
- `nakomi.studio` queda como marketing/entrada; `vps.nakomi.studio` como operación/producto.
- Deploy via `coolify-manager-rs`, no SSH directo.

---

## No hacer todavía

- No mover pagos de hosting/VPS sin auditar endpoints actuales.
- No exponer la GUI local tal cual en internet (sin auth).
- No permitir deploy/restart desde navegador sin auditoría y confirmaciones.
- No implementar compra directa de VPS hasta confirmar proveedor, costos y flujo antifraude.
- No usar `LOCAL_MODE=true` en producción.

---

## Referencias

- Plan anterior: `plan-vps-nakomi-studio-2026-05-10.md` (reemplazado por este)
- Arquitectura técnica: `Agente/documentacion/vps/arquitectura-vps-nakomi-studio-2026-05-10.md`
- Nakomi Studio repo: `glory-rust-template` (rama `glory-rust-nakomi`)
- Código VPS reutilizable: `src/handlers/vps.rs`, `src/services/contabo.rs`, `src/services/vps_stripe.rs`, `src/models/vps.rs`
- Frontend VPS actual: `frontend/src/islands/SolucionVpsIsland.tsx`
- Planes relacionados: `plan-hosting-automation-2026-04-10.md`, `plan-dominios-2026-04-07.md`
