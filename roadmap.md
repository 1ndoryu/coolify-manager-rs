# coolify-manager-rs — Roadmap

> **Descripción:** Herramienta de gestión para sitios Coolify — CLI + MCP Server + GUI web + portal vps.nakomi.studio
> **Stack:** Rust/Axum (backend) + React/Vite/TypeScript (frontend GUI)
> **Repositorio:** github.com/1ndoryu/coolify-manager-rs (rama `main`)
> **Deploy:** Coolify — requiere aprobación explícita del operador antes de ejecutar
> **Plan activo:** `Agente/planes/plan-vps-nakomi-studio-2026-05-12.md`

## Herramientas del agente
- coolify-manager-rs (este proyecto), code-sentinel, varsense (ver protocolo sección VII)

## Tareas pendientes

## Mejoras pendientes (268A-5, verificadas en despliegue real de agape)

- **E11 rollback ciego a HTTP:** `deploy-service` en un sitio NUEVO sin DNS configurado falla el health
  check HTTPS (`https://dominio/api/health` no resuelve) y entra en bucle rollback→rebuild (~10 min
  por ciclo) aunque el contenedor esté healthy y `/api/health` interno responda 200. Mejora:
  distinguir "dominio no resuelve aún" (warning, no rollback) de "app rota" (rollback). Idea:
  verificar resolución DNS del FQDN antes de tratar el fallo HTTP como fatal, o usar la URL interna
  (sslip.io / IP del contenedor) como health primario cuando el DNS del dominio aún no apunte.

## Incidente 2026-08-27: limpieza global de contenedores exited (CORREGIDO, commit eb1ce73)

- **Síntoma:** el deploy de `task` (stack `j4skk8...`) tumbó los otros 9 sitios (5 WordPress + 4 Rust).
- **Causa raíz:** el bloque `[04A-1]` de `deploy_service.rs` limpiaba contenedores exited con
  `docker ps -a --filter status=exited` + `docker rm {name}` **sin filtrar por stack**, borrando
  contenedores de TODOS los sitios del host.
- **Impacto:** 9 sitios caídos (503) — contenedores borrados, datos intactos (docker rm no toca
  volúmenes ni imágenes). Ninguna pérdida de datos verificada.
- **Fix aplicado (commit `eb1ce73`):** la limpieza ahora filtra por
  `label=coolify.stack-uuid={uuid}` (mismo patrón que `diagnose.rs`), solo toca contenedores del
  stack objetivo. Test de regresión `cleanup_exited_cmd_filters_by_stack_uuid` añadido.
- **Lección:** toda operación de limpieza/búsqueda de contenedores en producción DEBE filtrar por
  `label=coolify.stack-uuid={uuid}`. Nunca `docker ps -a` global.
- **Pendiente opcional:** revisar `docker_host_cleanup_manager.rs` y `target_bootstrap_manager.rs`
  (también usan `docker ps -a` global) para confirmar que su alcance es intencional (limpieza
  explícita de host) y no un riesgo similar.

### Fase 2 — Deploy online (BLOQUEADO — requiere supervisión del operador)

- 105A-34: Despliegue `vps.nakomi.studio` — **NO ejecutar sin aprobación explícita del operador**
  - Prerrequisitos completados: 125A-1, 125A-2, 125A-3
  - Prerrequisito pendiente: revisión local por el operador

### Fase 3 — MVP online seguro (post-deploy)

- 105A-36: RBAC + auditoría — roles admin/operator/viewer, tabla de eventos
- 105A-42: API read-only con DTOs seguros — sin paths, tokens ni config cruda
- 105A-44: Permisos write + auditoría completa de eventos

### Fase 4 — Portal VPS (post-deploy)

- 105A-37: Portal VPS conectado a API de Nakomi — panel cliente + panel admin
