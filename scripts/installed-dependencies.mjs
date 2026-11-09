// HS2-PNBH4X: the local gate must not issue or reuse receipts from an installed dependency tree that
// differs from the committed lockfile. npm records what it actually installed in
// node_modules/.package-lock.json (the hidden lockfile); comparing it with package-lock.json is a
// deterministic, offline check. A worktree that advanced its lockfile without `npm ci` fails here.
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const MANIFEST_FIELDS = ['dependencies', 'devDependencies', 'optionalDependencies', 'peerDependencies'];

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

function sameFields(left = {}, right = {}) {
  return JSON.stringify(Object.entries(left).sort()) === JSON.stringify(Object.entries(right).sort());
}

/**
 * Compare one Node package's install with its committed lockfile. Returns null for a directory
 * without a package-lock.json (nothing to verify), otherwise `{ problems, digest }`: `problems` is
 * empty only when the installed tree is exactly the locked one, and `digest` names that installed
 * tree so receipts made from it cannot be reused for another.
 */
export function installedDependencies(packageDir) {
  const lockPath = join(packageDir, 'package-lock.json');
  if (!existsSync(lockPath)) return null;
  const lock = readJson(lockPath);
  const problems = [];
  const manifestPath = join(packageDir, 'package.json');
  if (existsSync(manifestPath)) {
    const manifest = readJson(manifestPath);
    const root = lock.packages?.[''] ?? {};
    for (const field of MANIFEST_FIELDS)
      if (!sameFields(manifest[field], root[field]))
        problems.push(`package.json ${field} differs from package-lock.json`);
  }
  const hiddenPath = join(packageDir, 'node_modules', '.package-lock.json');
  if (!existsSync(hiddenPath)) {
    problems.push('dependencies are not installed (node_modules/.package-lock.json is missing)');
    return { problems, digest: null };
  }
  const hiddenText = readFileSync(hiddenPath, 'utf8');
  const installed = JSON.parse(hiddenText).packages ?? {};
  const locked = lock.packages ?? {};
  for (const [path, entry] of Object.entries(locked)) {
    if (path === '' || entry.link) continue;
    const actual = installed[path];
    // An optional package for another platform is legitimately absent.
    if (!actual) {
      if (!entry.optional) problems.push(`${path} ${entry.version ?? ''} is locked but not installed`.trim());
    } else if (actual.version !== entry.version)
      problems.push(`${path} is installed at ${actual.version} but locked at ${entry.version}`);
  }
  for (const path of Object.keys(installed))
    if (!(path in locked)) problems.push(`${path} is installed but not in package-lock.json`);
  return { problems, digest: createHash('sha256').update(hiddenText).digest('hex') };
}

/**
 * Verify every Node package directory a gate selection runs in. Throws with the exact fix when any
 * install is missing or stale; otherwise returns the installed-tree digests to bind into receipts.
 */
export function verifyInstalledDependencies(repo, directories) {
  const digests = {};
  const failures = [];
  for (const directory of [...new Set(directories)].sort()) {
    const state = installedDependencies(join(repo, directory));
    if (!state) continue;
    if (state.problems.length) {
      const shown = state.problems.slice(0, 5).map((problem) => `  - ${problem}`);
      if (state.problems.length > 5) shown.push(`  - ...and ${state.problems.length - 5} more`);
      failures.push(
        `${directory}: installed dependencies do not match the committed lockfile\n${shown.join('\n')}\n  Fix: (cd ${directory} && npm ci)`,
      );
    } else digests[directory] = state.digest;
  }
  if (failures.length) throw new Error(`Stale or missing installed dependencies (HS2-PNBH4X):\n${failures.join('\n')}`);
  return digests;
}

// HS2-R0XY7S: rebuild scripts run this before installing Rust binaries or stopping the server.
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 3 || process.argv[2] !== '--check') {
    process.stderr.write('Usage: node scripts/installed-dependencies.mjs --check\n');
    process.exitCode = 2;
  } else {
    try {
      verifyInstalledDependencies(process.cwd(), ['.']);
    } catch (error) {
      process.stderr.write(`${error.message}\n`);
      process.exitCode = 1;
    }
  }
}
