# Guia MCP para VS Code

## Requisitos

- Binario compilado de coolify-manager-rs.
- Archivo config/settings.json válido.
- Acceso SSH a la VPS o targets configurados.

## Compilar

```powershell
cargo build --release
```

## Configurar en este workspace

Crear o actualizar `.vscode/mcp.json` con:

```json
{
  "servers": {
    "coolify-manager": {
      "type": "stdio",
      "command": "${workspaceFolder}/.agent/coolify-manager-rs/target/release/coolify-manager.exe",
      "args": ["--mcp"]
    }
  }
}
```

## Verificar que responde

1. Reiniciar VS Code o recargar la ventana.
2. Confirmar que el servidor MCP aparece activo.
3. Probar una tool no destructiva primero, por ejemplo:
   - `coolify_list_sites`
   - `coolify_health`
   - `coolify_backup` con `list=true`

## Flujo recomendado de prueba

1. Ejecutar `coolify_list_sites` para validar configuración.
2. Ejecutar `coolify_health` sobre un sitio existente.
3. Ejecutar `coolify_backup` con tier `manual`.
4. Ejecutar `coolify_backup` con `list=true` para confirmar el backup.
5. Ejecutar `coolify_audit_vps` para comprobar conectividad SSH.
6. Ejecutar `coolify_migrate` con `dry_run=true` antes de cualquier migración real.

## Tools nuevas relevantes

- `coolify_backup`: crea o lista snapshots externos por sitio.
- `coolify_restore_backup`: restaura solo backups listos y validados.
- `coolify_health`: ejecuta chequeos HTTP, internos y de logs.
- `coolify_migrate`: migra un sitio completo hacia un target nombrado.
- `coolify_audit_vps`: audita carga, memoria, disco, Docker y señales básicas de seguridad.
- `coolify_wp_security`: revisa WordPress y puede rotar la password de un admin.

## Notas operativas

- Los backups se guardan en `backupStorage.localDir`, nunca en la VPS remota.
- Para migración hace falta `targets[]` en settings.json.
- Si la VPS todavía no tiene llave, puede usarse `sshPassword` en `vps` o en cada entrada de `targets`.
- El restore crea snapshot previo salvo que se use `skip_safety_snapshot=true`.
- El update protegido del tema usa backup previo + rollback git + health check final.