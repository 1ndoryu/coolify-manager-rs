# coolify-manager-rs — instrucciones del agente

CLI Rust que centraliza toda operación contra Coolify (deploy, health, backup/restore, logs, exec,
sync-env, creación de sitios, DNS, migración/failover). Es la **única herramienta autorizada** para
operaciones remotas de producción: SSH directo, Docker remoto, SCP, curl a la API y mutaciones
laterales están prohibidos. Escrituras remotas requieren autorización explícita por operación+objetivo.

## Build y binario

- **Repo (canónico):** este directorio (`area-trabajo/coolify-manager-rs`), rama `main`.
  El origen remoto es `github.com/1ndoryu/coolify-manager-rs`.
- **Regla de compilación (área-trabajo):** todo build va a `C:\tmp`, nunca a `target` dentro del árbol.
- Comando de build:

```powershell
cd "C:\Users\Owner\OneDrive\Documentos\area-trabajo\coolify-manager-rs"
$env:CARGO_TARGET_DIR = "C:\tmp\glory-target\coolify-manager"
cargo build --release
```

- **Binario resultante:** `C:\tmp\glory-target\coolify-manager\release\coolify-manager.exe`
  (también aparece como `release\deps\coolify_manager.exe`; es el mismo binario).
- Config por defecto: `config/settings.json` (gitignored, contiene secretos: apiToken, sshPassword,
  etc.). Resolución: `-c/--config` > `COOLIFY_MANAGER_CONFIG` > ancestros del cwd > manifest dir.
  No exponer secretos en logs ni commits.
- La ubicación legacy `...\glorytemplate\.agent\coolify-manager-rs` es solo runtime instalado de
  backups programados (Task Scheduler). No editar ni recompilar ahí; la fuente canónica es esta carpeta.

## Gotchas críticos de Coolify (verificados)

1. **422 "docker_compose_raw should be base64 encoded" es engañoso (268A-5).** Coolify 4.0.0-beta.460
   valida el compose decodificado con `mb_detect_encoding($s, 'ASCII', true)` y devuelve ESE mensaje
   también cuando el contenido tiene bytes >127 (acentos, em-dash, BOM), aunque el base64 sea válido.
   Por eso:
   - `create_stack` y `update_stack_compose` sanean a ASCII puro con `template_engine::to_ascii_safe`
     antes de base64-encodear (tests de regresión incluidos).
   - Los templates `config/templates/*` deben permanecer ASCII puro; si se añade texto con acentos a
     un template/compose, el 422 vuelve. Usar `to_ascii_safe` no se salta: es obligatorio.
2. **`new --template rust`**: flags `--repo-url`, `--app-bin`, `--frontend-dir` para proyectos
   no-glory (p. ej. ong-agape → `ong-agape.git` / `ong-agame-backend` / `frontend-v2`). Sin ellos el
   render usa glory-rs / glory-backend / frontend. El template `rust-stack.yaml` usa `{{HEALTH_PATH}}`
   (default `/api/health`), `{{STACK_UUID}}` (placeholder que `new_site` reemplaza con el UUID real
   tras crear para `postgres-{uuid}`).
3. **`restart --all` PROHIBIDO** si hay workloads Rust en el VPS (deja todo en `exited`; lección
   2026-05-11). `deploy-service` NO es válido para Rust salvo `--skip-compose-sync` en caso 422 legacy.
4. **`exec`/`logs`**: verificar UUID+nombre; búsqueda por nombre solo puede conectar al contenedor
   equivocado. `logs --target app --lines 50` para stacks Rust.
5. **`postgres` hostname**: usar `postgres-{uuid}` en DATABASE_URL para evitar DNS collision con el
   postgres de coolify-db (28P01).
6. **Backups**: `backup`/`restore` con rotación 7 daily + 4 weekly; Coolify no respalda bind mounts.

## Flujo de trabajo

- Commits por bloque coherente en `main`, con mensaje en español y referencia al ID de tarea si aplica.
- Tests: `cargo test --lib` (con `CARGO_TARGET_DIR` en `C:\tmp`); mantener verde antes de cerrar.
- Documentación del proyecto: `README.md`, `Agente/` (roadmap, completados, planes).
- No inventar comandos ni flags: la autoridad es `& $cm --help` y `& $cm <comando> --help` del binario
  compilado. `cargo check`/`cargo test` completos se ejecutan con el target en `C:\tmp`.
