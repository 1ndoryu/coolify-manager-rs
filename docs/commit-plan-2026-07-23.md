# Plan de Commits — coolify-manager-rs (2026-07-23)

> **Objetivo:** Commitear todos los cambios pendientes de forma ordenada, sin perder nada.
> **Estado:** En progreso

## Contexto

El repositorio tiene ~109 archivos sin commitear:
- **91 archivos** con solo cambios de línea (LF→CRLF)
- **11 archivos** con cambios reales de contenido
- **7 archivos nuevos** sin trackear

## Orden de commits

| # | Descripción | Archivos | Estado |
|---|---|---|---|
| 1 | `chore: normalize line endings (LF→CRLF)` | ~91 archivos | ⬜ |
| 2 | `feat: add cloudflare_api and docker_api infra modules` | 2 nuevos | ⬜ |
| 3 | `feat: add diagnose, host-exec, install-backups, setup-site-dns commands` | 4 nuevos | ⬜ |
| 4 | `feat: add backup-server.sh for server-side automation` | 1 nuevo | ⬜ |
| 5 | `feat: update deps and extend domain/error/config types` | 4 modificados | ⬜ |
| 6 | `feat: extend API endpoints and CLI dispatch` | 2 modificados | ⬜ |
| 7 | `feat: expand dns_manager with Cloudflare integration` | 1 modificado | ⬜ |
| 8 | `feat: add new MCP tools and GUI API endpoint` | 2 modificados | ⬜ |
| 9 | `docs: update README with db-check, db-migrate, run-sql, restore-client` | 1 modificado | ⬜ |
| 10 | Verificar: `git status` limpio | — | ⬜ |

## Notas

- Los commits 1-4 son "seguros" — no afectan código existente
- Los commits 5-8 son cambios de contenido real
- El commit 9 es solo documentación
- Todos los commits se hacen con `git add` explícito (nunca `git add .`)
