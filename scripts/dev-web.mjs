/* dev-web.mjs — Launcher de desarrollo para coolify-manager-rs (GUI web).
 * Equivalente al dev.mjs de glory-rs: sccache, CARGO_TARGET_DIR por rama,
 * auto-limpieza del target, sincronizacion de deps del GUI y deteccion de puertos libres.
 * No gestiona BD ni migraciones porque esta herramienta no las necesita. */

import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import net from "node:net";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const projectRoot = resolve(process.env.COOLIFY_MANAGER_DEV_ROOT || process.cwd());
const guiDir = resolve(projectRoot, "gui");
const guiPackageJson = resolve(guiDir, "package.json");
const guiPackageLock = resolve(guiDir, "package-lock.json");
const guiNodeModules = resolve(guiDir, "node_modules");
const guiInstallMarker = resolve(guiNodeModules, ".dev-web-install.json");

const isWindows = process.platform === "win32";
const cargoTargetBase = process.env.CARGO_TARGET_DIR_BASE || (isWindows ? "C:\\tmp\\glory-target" : resolve(tmpdir(), "glory-target"));
const cargoTargetMaxMb = Number(process.env.GLORY_CARGO_TARGET_MAX_MB || "4096");
const cargoCleanIntervalSeconds = Number(process.env.GLORY_CARGO_CLEAN_INTERVAL_SECONDS || "120");

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

function start(label, command, args, env) {
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

/* ---------- deteccion de rama ---------- */

function runGit(args) {
  const result = spawnSync(isWindows ? "git.exe" : "git", args, { cwd: projectRoot, encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : "";
}

function slugifyBranch(branch) {
  return branch.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "") || "local";
}

function detectBranch() {
  return runGit(["branch", "--show-current"]) || runGit(["rev-parse", "--short", "HEAD"]) || "local";
}

/* ---------- sccache ---------- */

function resolveRustcWrapper() {
  if (process.env.RUSTC_WRAPPER) return process.env.RUSTC_WRAPPER;
  if (isWindows) {
    const userProfile = process.env.USERPROFILE;
    if (userProfile) {
      const candidate = resolve(userProfile, ".cargo", "bin", "sccache.exe");
      if (existsSync(candidate)) return candidate;
    }
  }
  const probe = spawnSync("sccache", ["--version"], { encoding: "utf8", shell: isWindows });
  return probe.status === 0 ? "sccache" : null;
}

/* ---------- sincronizacion de deps del GUI ---------- */

function hashFile(path) {
  if (!existsSync(path)) return null;
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function guiDepFingerprint() {
  return { packageJson: hashFile(guiPackageJson), packageLock: hashFile(guiPackageLock) };
}

function ensureGuiDeps() {
  if (process.env.DEV_SKIP_GUI_INSTALL === "1") return;
  const fp = guiDepFingerprint();
  if (existsSync(guiNodeModules) && existsSync(guiInstallMarker)) {
    try {
      const saved = JSON.parse(readFileSync(guiInstallMarker, "utf8"));
      if (saved.packageJson === fp.packageJson && saved.packageLock === fp.packageLock) return;
    } catch { /* no-op */ }
  }
  console.log("[dev-web] Dependencias GUI desfasadas; ejecutando npm install...");
  /* En Windows los archivos .cmd no se pueden spawn directamente sin shell;
   * hay que envolverlos en cmd.exe /d /s /c igual que spawnPortable. */
  const npmArgs = ["install", "--no-audit", "--no-fund"];
  const syncInvocation = isWindows
    ? { cmd: process.env.ComSpec || "cmd.exe", args: ["/d", "/s", "/c", ["npm.cmd", ...npmArgs].map(quoteWindowsArg).join(" ")] }
    : { cmd: "npm", args: npmArgs };
  const result = spawnSync(syncInvocation.cmd, syncInvocation.args, {
    cwd: guiDir, env: process.env, stdio: "inherit",
  });
  if (result.status !== 0) {
    console.error("[dev-web] No se pudieron sincronizar las dependencias del GUI.");
    process.exit(result.status ?? 1);
  }
  writeFileSync(guiInstallMarker, JSON.stringify({ ...guiDepFingerprint(), updatedAt: new Date().toISOString() }, null, 2));
  console.log("[dev-web] Dependencias GUI sincronizadas.");
}

/* ---------- auto-limpieza del target ---------- */

function spawnCargoTargetWatcher(env, activeTargetDir) {
  if (!isWindows) return;
  const watcherScript = resolve(scriptDir, "watch-cargo-target.ps1");
  if (!existsSync(watcherScript)) return;
  const child = spawn(
    "powershell.exe",
    ["-ExecutionPolicy", "Bypass", "-File", watcherScript,
      "-TargetDirs", cargoTargetBase,
      "-ExcludeDirs", activeTargetDir,
      "-MaxTotalMB", String(cargoTargetMaxMb),
      "-IntervalSeconds", String(cargoCleanIntervalSeconds)],
    { cwd: projectRoot, env, stdio: "ignore" },
  );
  child.on("error", () => { /* watcher opcional; no bloquea */ });
  children.push(child);
}

ensureGuiDeps();

const branch = detectBranch();
const cargoTargetDir = process.env.CARGO_TARGET_DIR || resolve(cargoTargetBase, `coolify_manager_${slugifyBranch(branch)}`);
const rustcWrapper = resolveRustcWrapper();

const apiHost = requestedApiUrl.hostname || "127.0.0.1";
const apiPortBase = Number(requestedApiUrl.port || 8787);
const viteHost = process.env.VITE_HOST ?? "127.0.0.1";
const vitePort = await puertoLibre(viteHost, Number(process.env.VITE_PORT ?? 5173));
const apiPort = process.env.VITE_COOLIFY_MANAGER_API_URL ? apiPortBase : await puertoLibre(apiHost, apiPortBase);
requestedApiUrl.port = String(apiPort);
const apiUrl = requestedApiUrl.toString().replace(/\/$/, "");

const childEnv = {
  ...process.env,
  VITE_COOLIFY_MANAGER_API_URL: apiUrl,
  CARGO_TARGET_DIR: cargoTargetDir,
  ...(rustcWrapper ? { RUSTC_WRAPPER: rustcWrapper } : {}),
};

console.log(`[dev-web] Rama: ${branch}`);
console.log(`[dev-web] Cargo target: ${cargoTargetDir}`);
if (rustcWrapper) console.log(`[dev-web] Rust cache: ${rustcWrapper}`);
console.log(`[dev-web] API local real: ${apiUrl}`);
console.log(`[dev-web] Vite: http://${viteHost}:${vitePort}\n`);

start("gui-api", "cargo", ["run", "--", "gui-api", "--bind", `${apiHost}:${apiPort}`], childEnv);
start("vite", isWindows ? "npm.cmd" : "npm", ["--prefix", "gui", "run", "dev:web", "--", "--host", viteHost, "--port", String(vitePort)], childEnv);
spawnCargoTargetWatcher(childEnv, cargoTargetDir);
