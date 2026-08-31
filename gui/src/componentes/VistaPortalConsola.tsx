/*
 * [125A-5] ConsoleOverlay — consola de codigo superpuesta al hero de VistaPortal.
 * Extraido de VistaPortal.tsx (Fase H): sub-componente visual sin estado.
 */

export function ConsoleOverlay() {
    return (
        <div className="vpsConsole">
            <div className="vpsConsoleBar">
                <span />
                <span />
                <span />
                <p>vps.nakomi.studio</p>
            </div>
            <pre className="vpsConsoleCode">
                <code>{`... async function deploy({ service }) {
  const health = await coolify.health(service)

  if (!health.ok) {
    await backups.restoreLatest(service)
    return { status: "restored" }
  }

  await docker.compose.pull(service)
  await docker.compose.up(service)

  return { status: "online" }
} ...`}</code>
            </pre>
        </div>
    );
}
