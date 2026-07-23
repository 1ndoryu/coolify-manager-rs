# Plan de Commits — coolify-manager-rs (2026-07-23)

> **Objetivo:** Commitear todos los cambios pendientes de forma ordenada, sin perder nada.
> **Estado:** ✅ COMPLETADO

## Contexto

El repositorio tenía ~109 archivos sin commitear:
- **91 archivos** con solo cambios de línea (LF→CRLF)
- **11 archivos** con cambios reales de contenido
- **7 archivos nuevos** sin trackear

## Commits realizados

| # | Hash | Descripción | Archivos |
|---|---|---|---|
| 1 | `b42ded9` | `feat: add run-sql, db-check, db-migrate, restore-client commands + shared pg_utils` | 8 nuevos (4 commands + pg_utils + mod.rs + cli/mod.rs + dispatch/ops.rs) |
| 2 | `3708039` | `fix: route RunSql/DbCheck/DbMigrate/RestoreClient to ops dispatch` | 1 modificado (dispatch.rs) |
| 3 | `1f28115` | `chore: normalize line endings (LF->CRLF) + remaining uncommitted changes` | 119 archivos (91 line-endings + 11 content changes + 7 untracked + docs) |

## Resumen de cambios incluidos en el commit 3

### Archivos con cambios reales de contenido (11)
- `Cargo.lock` — nuevas dependencias
- `Cargo.toml` — nuevas dependencias
- `README.md` — documentación de db-check, db-migrate, run-sql, restore-client
- `src/api/mod.rs` — nuevos endpoints API
- `src/cli/dispatch/site.rs` — dispatch extensions
- `src/config/mod.rs` — configuración extendida
- `src/domain/mod.rs` — tipos de dominio nuevos
- `src/error/mod.rs` — tipos de error nuevos
- `src/gui_api.rs` — endpoint GUI API
- `src/mcp/tools.rs` — nuevas herramientas MCP
- `src/services/dns_manager.rs` — integración Cloudflare

### Archivos nuevos sin trackear (7)
- `scripts/backup-server.sh` — script de backup server-side
- `src/commands/diagnose.rs` — comando de diagnóstico
- `src/commands/host_exec.rs` — ejecutar comandos en host
- `src/commands/install_backups.rs` — instalar backups automáticos
- `src/commands/setup_site_dns.rs` — configurar DNS
- `src/infra/cloudflare_api.rs` — API de Cloudflare
- `src/infra/docker_api.rs` — API Docker Engine

### Archivos con solo cambios de línea (91)
Normalización LF→CRLF en todo el proyecto.

## Documentación

- **README.md** — Ya documenta db-check, db-migrate, run-sql, restore-client ✅
- **Skill coolify-manager** — Necesita actualización con los nuevos comandos (pendiente: el archivo del skill está gestionado por `npx skills` y no se encontró en el directorio del proyecto)

## Verificación final

```
$ git status
On branch main
nothing to commit, working tree clean

$ git log --oneline -5
1f28115 chore: normalize line endings (LF->CRLF) + remaining uncommitted changes
3708039 fix: route RunSql/DbCheck/DbMigrate/RestoreClient to ops dispatch
b42ded9 feat: add run-sql, db-check, db-migrate, restore-client commands + shared pg_utils
6de80b6 21C-7: inject pg_data volume en postgres si falta
8260faa 21C-6: fix build-arg REPO_URL explicito
```

✅ Working tree clean. Todos los cambios commiteados y pushed.
