# coolify-manager-rs — Roadmap

> **Descripción:** Herramienta de gestión para sitios Coolify — CLI + MCP Server + GUI web + portal vps.nakomi.studio
> **Stack:** Rust/Axum (backend) + React/Vite/TypeScript (frontend GUI)
> **Repositorio:** github.com/1ndoryu/coolify-manager-rs (rama `main`)
> **Deploy:** Coolify — requiere aprobación explícita del operador antes de ejecutar
> **Plan activo:** `Agente/planes/plan-vps-nakomi-studio-2026-05-12.md`

## Herramientas del agente
- coolify-manager-rs (este proyecto), code-sentinel, varsense (ver protocolo sección VII)

## Tareas pendientes

### Fase 1 — UX GUI operativa

- 105A-29: Selector global de VPS en logoSidebar — usar `SelectorPersonalizado`, no `<select>` nativo
- 105A-31: "Agregar sitio" como modal funcional — abrir `ModalAgregarSitio` sin navegar a Ajustes
- 105A-32: Retirar `rutaPagina` de las vistas — ajustar espaciado del header
- 105A-33: Favicons inline para sitios en tabla — fallback determinista si no carga
- 105A-30: Regla Sentinel contra `<select>` nativo en React/TSX

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
