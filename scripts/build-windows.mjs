#!/usr/bin/env node
/* build-windows.mjs — genera coolify-manager.exe en la VPS 89 (cross-compile).
 *
 * [por que] El portátil dejó de compilar Rust el 2026-10-16 (AGENTS §1): la
 * toolchain se retiró entera. El .exe es de Windows, pero compilarlo cruzado con
 * cargo-xwin en la 89 deja el `target/` fuera del portátil y el binario se
 * ejecuta en Windows igual. Mismo patrón que limpiador-pc/scripts/build-windows.mjs.
 *
 * coolify-manager-rs no tiene sesión Mutagen (su botón del taller es la web, no el
 * binario), así que el transporte lo hace este script: fuente -> 89 -> .exe de
 * vuelta. La copia remota vive en /var/tmp (la barre cm-sweep); lo persistente es
 * el toolchain de la 89 (~/.cargo, ~/.cache/cargo-xwin).
 *
 * Uso: node scripts/build-windows.mjs [--salida <ruta.exe>] [--host dev89]
 */
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const PROYECTO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const TRIPLE = 'x86_64-pc-windows-msvc';
const NOMBRE_EXE = 'coolify-manager.exe';
const SSH_OPTS = ['-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10', '-o', 'ServerAliveInterval=30'];
/* Todo bajo /var/tmp: fuente y target son desechables, la 89 los barre sola. */
const BASE_REMOTA = '/var/tmp/coolify-manager-rs';
const FUENTE_REMOTA = `${BASE_REMOTA}/fuente`;
const TARGET_REMOTO = `${BASE_REMOTA}/target`;
/* CRT estático: el .exe no depende de la redistribuible de Visual C++. */
const RUSTFLAGS = '-C target-feature=+crt-static';
/* Lo que NO viaja. `./.cargo` fuera porque su config.toml fija
 * `target-dir = "C:/tmp/glory-target/..."`, una ruta de Windows que en Linux
 * sería un directorio relativo absurdo; CARGO_TARGET_DIR manda de todos modos.
 * `settings.json` y sus copias fuera porque llevan credenciales (AGENTS §2): el
 * build solo necesita `config/templates/`, que `contexto.rs` incrusta con
 * include_str!. Anclados con `./` para no excluir de más. */
const EXCLUIR = [
  './.git',
  './target',
  './.cargo',
  './node_modules',
  './dist',
  './logs',
  './.quality-reports',
  './.sentinel',
  './.env',
  './.env.*',
  './config/settings.json',
  './config/settings.json.*',
  '*/node_modules',
  '*/target',
];

function arg(nombre, porDefecto) {
  const i = process.argv.indexOf(`--${nombre}`);
  return i >= 0 && process.argv[i + 1] ? process.argv[i + 1] : porDefecto;
}

const HOST = arg('host', 'dev89');
const SALIDA = path.resolve(arg('salida', 'C:/tmp/coolify-manager-rs/' + NOMBRE_EXE));
if (!/^[a-zA-Z0-9._-]+$/.test(HOST)) {
  console.error(`host inválido: ${HOST}`);
  process.exit(2);
}

/* `tar` del PATH puede ser el de Windows (bsdtar) según cómo arranque el padre;
 * el de Git es el que se ha probado. Se resuelve por ruta, no por orden. */
function tarReal() {
  const candidatos = ['C:\\Program Files\\Git\\usr\\bin\\tar.exe', 'C:\\Program Files\\Git\\bin\\tar.exe'];
  for (const c of candidatos) if (fs.existsSync(c)) return c;
  return 'tar';
}

function morir(msg, detalle) {
  console.error(`\n[build-windows] ${msg}`);
  if (detalle) console.error(detalle.trimEnd());
  process.exit(1);
}

/* Envía la fuente (solo archivos del proyecto, sin lo excluido) por stdin de ssh. */
function enviarFuente() {
  const tar = spawn(tarReal(), ['czf', '-', ...EXCLUIR.flatMap((e) => [`--exclude=${e}`]), '-C', PROYECTO, '.']);
  const ssh = spawn(
    'ssh',
    [...SSH_OPTS, HOST, `rm -rf ${FUENTE_REMOTA} && mkdir -p ${FUENTE_REMOTA} && tar xzmf - -C ${FUENTE_REMOTA}`],
    { stdio: ['pipe', 'inherit', 'inherit'] },
  );
  tar.stdout.pipe(ssh.stdin);
  /* Si ssh muere antes de tragarse el tar, el pipe revienta con EPIPE. */
  ssh.stdin.on('error', () => {});
  return new Promise((ok, mal) => {
    tar.on('error', (e) => mal(new Error(`no se pudo ejecutar tar: ${e.message}`)));
    ssh.on('error', (e) => mal(new Error(`no se pudo ejecutar ssh: ${e.message}`)));
    ssh.on('close', (c) => (c === 0 ? ok() : mal(new Error(`el envío de la fuente falló (ssh ${c})`))));
  });
}

/* Compila en la 89 bajo cm-heavy (un build pesado a la vez, con tope de RAM y
 * disco). `cargo` no está en el PATH de ssh no interactivo: ruta absoluta.
 * `~/.local/bin` va delante porque cc-rs (el C de `ring`) busca el archivador
 * por su nombre sin versión: en Ubuntu solo existe `/usr/bin/llvm-lib-18`, y
 * ahí hay un enlace `llvm-lib` -> `llvm-lib-18`. Sin él el build muere con
 * «failed to find tool "llvm-lib"». */
function compilarRemoto() {
  const cmd =
    `cd ${FUENTE_REMOTA} && PATH=$HOME/.local/bin:$PATH CARGO_TARGET_DIR=${TARGET_REMOTO} ` +
    `RUSTFLAGS='${RUSTFLAGS}' ` +
    `cm-heavy "$HOME/.cargo/bin/cargo" xwin build --release --target ${TRIPLE}`;
  const r = spawnSync('ssh', [...SSH_OPTS, HOST, cmd], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (r.error) morir(`no se pudo ejecutar ssh: ${r.error.message}`);
  const salida = `${r.stdout ?? ''}${r.stderr ?? ''}`;
  if (r.status === 75) morir('la 89 no tiene recursos ahora (cm-heavy, exit 75); reintentar más tarde', salida);
  if (r.status !== 0) morir(`la compilación en la 89 falló (exit ${r.status})`, salida);
  return salida;
}

/* Trae un archivo remoto por stdout de ssh. Evita `scp` porque en Windows una
 * ruta local con dos puntos (`C:\...`) se confunde con host:ruta. */
function traer(remoto, local) {
  const r = spawnSync('ssh', [...SSH_OPTS, HOST, `cat ${remoto}`], { maxBuffer: 64 * 1024 * 1024 });
  if (r.error) morir(`no se pudo ejecutar ssh: ${r.error.message}`);
  if (r.status !== 0) morir(`no se pudo leer ${remoto} de la 89 (exit ${r.status})`);
  fs.mkdirSync(path.dirname(local), { recursive: true });
  fs.writeFileSync(local, r.stdout);
  return r.stdout.length;
}

/* Cargo.lock: la 89 puede haberlo actualizado (dependencia nueva sin build
 * local que lo fije). Se trae solo si cambió, como haría un build en el laptop. */
function sincronizarLock() {
  const local = path.join(PROYECTO, 'Cargo.lock');
  const r = spawnSync('ssh', [...SSH_OPTS, HOST, `cat ${FUENTE_REMOTA}/Cargo.lock`], { maxBuffer: 16 * 1024 * 1024 });
  if (r.status !== 0 || !r.stdout?.length) return false;
  const actual = fs.existsSync(local) ? fs.readFileSync(local) : Buffer.alloc(0);
  if (r.stdout.equals(actual)) return false;
  fs.writeFileSync(local, r.stdout);
  return true;
}

const bytes = await (async () => {
  console.error(`[build-windows] enviando fuente a ${HOST}…`);
  await enviarFuente();
  console.error(`[build-windows] compilando ${TRIPLE} en la 89 (cm-heavy)…`);
  const log = compilarRemoto();
  const ultima = log.trimEnd().split('\n').at(-1) ?? '';
  console.error(`[build-windows] ${ultima}`);
  const n = traer(`${TARGET_REMOTO}/${TRIPLE}/release/${NOMBRE_EXE}`, SALIDA);
  if (n < 1024 || fs.readFileSync(SALIDA).subarray(0, 2).toString('latin1') !== 'MZ') {
    morir(`el artefacto descargado no es un ejecutable de Windows (${n} B)`);
  }
  if (sincronizarLock()) console.error('[build-windows] Cargo.lock actualizado en la 89 y traído al proyecto');
  return n;
})();

console.error(`[build-windows] ok: ${SALIDA} (${bytes} B)`);
console.log(`exe: ${SALIDA} (${bytes} B)`);
