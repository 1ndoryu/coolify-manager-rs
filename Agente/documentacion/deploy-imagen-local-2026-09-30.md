# Deploy desde imagen local (sin build en el VPS) — 2026-09-30

> Fuente: tarea `309A-1` (HECHA 01-10), plan
> `Agente/planes/completados/plan-build-laptop-2026-09-30.md`.
> La ayuda del binario (`--help`) es la autoridad sobre comandos y flags.

## 1. Terminología (leer primero)

La expresión **"build local" es ambigua y no debe usarse** en docs, código
nuevo ni skills. Significados históricos:

| Término canónico | Qué significa | Dónde ocurre el `docker build` |
|---|---|---|
| `build-laptop` (comando) | `build-laptop --name <sitio>` o `--dockerfile … --tag …` | Laptop Windows (Docker Desktop) |
| Build en VPS (ruta clásica) | `deploy-service` sin `--image` | En el VPS (único `build` remoto aceptado) |
| Pull desde registry (`119A-4`, aparcada) | `deploy-service` con `imageRef` de registry | GitHub Actions + registry (nunca en el VPS) |

Regla: en `redeploy.rs`, "build local" significa "local al VPS", **no** a la
laptop. Ante duda, nombrar el comando exacto (`build-laptop`,
`deploy-service --image`, `new --image`).

## 2. Matriz de rutas de deploy

| Ruta | Build | Transferencia | VPS hace | Estado |
|---|---|---|---|---|
| Clásica | En el VPS (`deploy-service` sin `--image`) | N/A (Dockerfile + contexto por API) | `build` + run | Vigente, intacta |
| `119A-4` registry | GitHub Actions | Pull desde GHCR | pull + run | **Aparcada** (código F1–F4 en `main`, ruta intacta) |
| `309A-1` laptop | Laptop (`build-laptop`) | `save → gzip → upload → load` | `load` + run | **Vigente (validada F4 01-10)** |

Ninguna ruta de prueba ejecuta `build` en el VPS salvo la clásica, que es el
comportamiento histórico aceptado (no un E2E de imagen).

## 3. Flujo `309A-1` (laptop → VPS)

```powershell
# 1. Compilar en la laptop, subir y cargar en el VPS (un solo comando)
& $cm build-laptop --name mi-rust
& $cm build-laptop --dockerfile config/templates/Dockerfile.kamples --tag cm-local/kamples:<sha12>

# 2. Crear el stack que referencia la imagen ya cargada
& $cm new --name mi-sitio --domain https://mi-sitio.invalid --template kamples --image cm-local/kamples:<sha12>

# 3. (Rust) sincronizar compose + usar imagen precargada sin build en VPS
& $cm deploy-service --name mi-rust --image cm-local/mi-rust:<sha12> --skip-backup
```

- Tag: `cm-local/<sitio>:<sha12>` (branch), **nunca `latest`**.
  `validate_image_ref` acepta `cm-local/` pero fail-closed sin pull
  (anti pull-hijack).
- `build-laptop` exige `df avail >= 2×tgz` en el VPS y verifica con
  `docker inspect` tras el `load`.
- Kamples + `--image` usa `instant_deploy=true` (`deploy-service` no soporta
  Kamples); el tema Glory se instala post-arranque (Paso 5), nunca horneado.

## 4. Evidencia F4 (01-10, desechable `cm-test-309a1k`)

- `Dockerfile.kamples` compilado en laptop (289 s), tgz 500.7 MB, `load`
  verificado por sha256 (`5df51a…`, `828278…`).
- 3/3 contenedores `Up`: wordpress (`cm-local/kamples:manual001`),
  mariadb:10.11 healthy, postgres pg17 healthy.
- WordPress HTTP 200; php 8.2 + `pdo_pgsql` + ffmpeg 7.1 + python 3.13;
  `npm install` + `npm run build` (SSG) OK con Node 20/npm 10.
- Limpieza: `delete-site` ×2 (2.º 404 verificado), 13 sitios intactos,
  `rmi cm-local/kamples:manual001` → VPS sin huella.

## 5. Límites conocidos (no reabrir sin su tarea)

- **01AA-1 (pendiente):** Coolify reescribe `MYSQL_PASSWORD` tras el create
  (`WORDPRESS_DB_PASSWORD` ≠ `MYSQL_PASSWORD` en disco pese a un único
  `{{DB_PASSWORD}}` renderizado; el par postgres llega intacto). Requiere
  reconciliación post-create. `restart` vía API en stacks solo-imagen deja
  WP en `Created` y pierde la imagen `cm-local` (Coolify intenta rebuild).
- **01AA-2 (pendiente, externo):** `composer install` de `glorytemplate`
  (sin `composer.lock`) bloqueado por advisory `PKSA-mh9b-91zm-m1gy`.
- `delete-site` necesita doble pasada (1.ª acepta DELETE pero valida
  "sigue existiendo"; 2.ª 404 + limpia). DNS `.invalid` sin zona: aviso
  esperado.
- Pin `pgvector/pgvector:pg17` solo en `kamples-image-stack.yaml` (el tag
  flotante pg18 cambió el layout de `PGDATA`); migrar ambos templates a
  pg18 es tarea separada.
- `npm install -g npm@latest` prohibido en `Dockerfile.kamples` (npm ≥12
  exige Node ≥22; basta npm 10 de Node 20).
- Builds largos: lectores stdout/stderr concurrentes (deadlock del pipe de
  64 KB si se drenan en serie); lanzador desatendido vía `Win32_Process`
  (`schtasks` da fantasmas); exe anclado en `C:\tmp\bin` (GloryTmpSweep
  purga `C:\tmp\glory-target` cada hora).
