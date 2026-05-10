import { spawn, spawnSync } from "node:child_process";
import net from "node:net";
import process from "node:process";

const isWindows = process.platform === "win32";
const npmCommand = "npm";
const cargoCommand = "cargo";
const requestedApiUrl = new URL(process.env.VITE_COOLIFY_MANAGER_API_URL ?? "http://127.0.0.1:8787");

const children = [];
let shuttingDown = false;

function portDisponible(host, port) {
  return new Promise((resolve) => {
    const server = net.createServer();
    server.once("error", () => resolve(false));
    server.once("listening", () => server.close(() => resolve(true)));
    server.listen(port, host);
  });
}

async function puertoLibre(host, initialPort) {
  for (let port = initialPort; port < initialPort + 40; port += 1) {
    if (await portDisponible(host, port)) return port;
  }
  throw new Error(`No hay puerto libre disponible desde ${initialPort}`);
}

function quoteWindowsArg(value) {
  const arg = String(value);
  if (/^[\w./:=@-]+$/.test(arg)) return arg;
  return `"${arg.replace(/"/g, '\\"')}"`;
}

function spawnPortable(command, args, env) {
  if (!isWindows) {
    return spawn(command, args, { stdio: "inherit", shell: false, env });
  }

  const commandLine = [command, ...args].map(quoteWindowsArg).join(" ");
  return spawn("cmd.exe", ["/d", "/s", "/c", commandLine], {
    stdio: "inherit",
    shell: false,
    env,
    windowsHide: false,
  });
}

function start(label, command, args, options = {}) {
  const env = {
    ...process.env,
    VITE_COOLIFY_MANAGER_API_URL: apiUrl,
    ...options.env,
  };
  const child = spawnPortable(command, args, env);

  children.push(child);
  child.on("error", (error) => {
    if (shuttingDown) return;
    console.error(`[dev-web] ${label} no pudo iniciar: ${error.message}`);
    shutdown(1);
  });
  child.on("exit", (code, signal) => {
    if (shuttingDown) return;
    console.log(`[dev-web] ${label} finalizo con code=${code ?? "null"} signal=${signal ?? "null"}`);
    shutdown(code ?? 1);
  });
  return child;
}

function shutdown(code = 0) {
  shuttingDown = true;
  for (const child of children) {
    if (child.killed || !child.pid) continue;
    if (isWindows) {
      spawnSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], { stdio: "ignore" });
    } else {
      child.kill();
    }
  }
  process.exit(code);
}

process.on("SIGINT", () => shutdown(0));
process.on("SIGTERM", () => shutdown(0));

const apiHost = requestedApiUrl.hostname || "127.0.0.1";
const apiPortBase = Number(requestedApiUrl.port || 8787);
const viteHost = process.env.VITE_HOST ?? "127.0.0.1";
const vitePort = await puertoLibre(viteHost, Number(process.env.VITE_PORT ?? 5173));
const apiPort = process.env.VITE_COOLIFY_MANAGER_API_URL ? apiPortBase : await puertoLibre(apiHost, apiPortBase);
requestedApiUrl.port = String(apiPort);
const apiUrl = requestedApiUrl.toString().replace(/\/$/, "");
const bind = `${apiHost}:${apiPort}`;

console.log(`[dev-web] API local real: ${apiUrl}`);
console.log(`[dev-web] Vite: http://${viteHost}:${vitePort}`);
start("gui-api", cargoCommand, ["run", "--", "gui-api", "--bind", bind]);
start("vite", npmCommand, ["--prefix", "gui", "run", "dev:web", "--", "--host", viteHost, "--port", String(vitePort)]);
