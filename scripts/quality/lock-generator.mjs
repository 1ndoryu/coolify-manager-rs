/* quality:lock — genera/valida sentinel.lock.json desde el checkout compartido.
 *
 * Port minimal del lock-generator que usan los consumidores migrados. Para
 * cada analyzer declarado en quality-tools.json inspecciona el checkout
 * instalado (provisionPath/sourcePath → compartido) y computa:
 *   version     = node <cli> --version
 *   commit      = git rev-parse HEAD del checkout fuente
 *   sha256      = sha256 de `git archive --format=tar HEAD`
 * y valida que el commit coincida con el fijado en quality-tools.json. Escribe
 * con backup previo. Nunca fabrica: si un commit descoincide o el CLI falta,
 * no escribe y falla.
 */
import { createHash } from 'node:crypto';
import { copyFile, mkdir, realpath, readFile, lstat, access } from 'node:fs/promises';
import { spawn, spawnSync } from 'node:child_process';
import path from 'node:path';

const LOCK_FILE = 'sentinel.lock.json';

function git(cwd, ...args) {
  const result = spawnSync('git', args, { cwd, encoding: 'utf8', windowsHide: true });
  if (result.error || result.status !== 0) return null;
  return result.stdout.trim();
}

function gitArchiveSha256(toolRoot) {
  return new Promise((resolve, reject) => {
    const child = spawn('git', ['-C', toolRoot, 'archive', '--format=tar', 'HEAD'], {
      cwd: toolRoot, shell: false, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
    });
    const hash = createHash('sha256');
    let stderr = '';
    const timeout = setTimeout(() => { child.kill('SIGTERM'); reject(new Error('git archive excedió el timeout')); }, 30000);
    child.stdout.on('data', c => hash.update(c));
    child.stderr.on('data', c => { stderr += c; });
    child.on('error', e => { clearTimeout(timeout); reject(e); });
    child.on('close', (code, signal) => {
      clearTimeout(timeout);
      if (signal || code !== 0) reject(new Error(`git archive falló (${signal ?? code}): ${stderr.trim()}`));
      else resolve(hash.digest('hex'));
    });
  });
}

function runNode(cliPath, args, cwd) {
  const result = spawnSync(process.execPath, [cliPath, ...args], { cwd, encoding: 'utf8', windowsHide: true, timeout: 20000 });
  if (result.error || result.status !== 0) return null;
  return result.stdout.trim();
}

function stableSerialize(value) {
  if (Array.isArray(value)) return `[${value.map(stableSerialize).join(',')}]`;
  if (value && typeof value === 'object') {
    return `{${Object.keys(value).sort().map(k => `${JSON.stringify(k)}:${stableSerialize(value[k])}`).join(',')}}`;
  }
  return JSON.stringify(value);
}

async function resolveConfiguredSourcePath(workspace, config) {
  // Prefiere sourcePath (externo → compartido); si es una ruta interna se mantiene.
  const configured = typeof config.sourcePath === 'string' ? config.sourcePath : null;
  if (!configured) return null;
  return realpath(path.resolve(workspace, configured));
}

async function inspectAnalyzers(workspace) {
  const manifest = JSON.parse(await readFile(path.join(workspace, 'quality-tools.json'), 'utf8'));
  const results = {};
  for (const [name, config] of Object.entries(manifest.tools || {})) {
    const toolRoot = path.resolve(workspace, config.provisionPath ?? '.quality-tools');
    const sourceRoot = (await resolveConfiguredSourcePath(workspace, config)) ?? toolRoot;
    if (!config.cli) throw new Error(`${name}: falta config.cli`);
    const cliPath = path.join(toolRoot, config.cli);
    try { await access(cliPath); } catch { throw new Error(`Falta el CLI provisionado de ${name}; ejecuta npm run quality:setup`); }

    const sumStatus = git(sourceRoot, 'status', '--porcelain') ?? '';
    const untrackedOrDirty = sumStatus.split(/\r?\n/).filter(Boolean).length;
    if (untrackedOrDirty > 0) throw new Error(`${name}: checkout fuente modificado; no se puede confiar en el lock`);

    const version = runNode(cliPath, ['--version'], workspace);
    if (version === null) throw new Error(`${name}: no se pudo leer la versión instalada`);
    const revision = git(sourceRoot, 'rev-parse', 'HEAD');
    if (!revision) throw new Error(`${name}: no se pudo leer el commit instalado`);
    if (config.commit && revision !== config.commit) {
      throw new Error(`${name}: checkout en ${revision} != commit fijado ${config.commit}`);
    }
    const sha256 = await gitArchiveSha256(sourceRoot);
    results[name] = {
      version,
      protocolVersion: Number(config.outputSchemaVersion ?? 1),
      commit: revision,
      sha256,
      patchSha256: config.patch?.sha256 ?? null,
      ...(config.capabilities === undefined ? {} : { capabilities: config.capabilities }),
    };
  }
  return results;
}

async function main(argv = process.argv.slice(2)) {
  const mode = argv.includes('--write') ? 'write' : 'check';
  const workspace = path.resolve(argv.includes('--cwd') ? argv[argv.indexOf('--cwd') + 1] : process.cwd());
  const analyzers = await inspectAnalyzers(workspace);
  const lock = {
    schemaVersion: 1,
    generatedAt: new Date().toISOString(),
    runtime: {
      status: 'project-adapter',
      version: '1.0.0-local',
      commit: 'repo-scripts',
      artifactSha256: null,
      identitySha256: createHash('sha256').update('sentinel-runtime:project-adapter:1.0.0-local:repo-scripts').digest('hex'),
    },
    analyzers,
  };
  const lockPath = path.join(workspace, LOCK_FILE);
  if (mode === 'check') {
    let actual;
    try { actual = JSON.parse(await readFile(lockPath, 'utf8')); } catch { console.error('[quality:lock] check: falta o inválido sentinel.lock.json'); process.exit(1); }
    const comparable = lock => ({ schemaVersion: lock.schemaVersion, analyzers: lock.analyzers });
    const ok = stableSerialize(comparable(actual)) === stableSerialize(comparable(lock)) && JSON.stringify(actual.analyzers) === JSON.stringify(lock.analyzers);
    console.log(`[quality:lock] ${ok ? 'pass' : 'mismatch'}`);
    process.exitCode = ok ? 0 : 1;
    return;
  }
  try {
    const metadata = await lstat(lockPath).catch(e => (e?.code === 'ENOENT' ? null : Promise.reject(e)));
    if (metadata && metadata.isSymbolicLink()) throw new Error('sentinel.lock.json: no se puede reemplazar un symlink');
    if (metadata) await copyFile(lockPath, `${lockPath}.bak`);
  } catch (e) { if (e?.code !== 'ENOENT') throw e; }
  await mkdir(path.dirname(lockPath), { recursive: true });
  await import('node:fs/promises').then(fs => fs.writeFile(lockPath, `${JSON.stringify(lock, null, 2)}\n`, { flag: 'w' }));
  console.log(`[quality:lock] written ${lockPath}`);
}

main().catch(err => { process.stderr.write(`[quality:lock] ERROR: ${err.message}\n`); process.exitCode = 2; });