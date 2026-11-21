// HS2-WC74CR: root `scripts/*.test.mjs` files that read repository files outside scripts/ as fixtures.
//
// The gate routes a changed path by package (scripts/gate-plan.mjs), so a root tooling test that reads,
// say, a Rust source as a live fixture was not selected when only that source changed (HS2-3RTF0G).
// The tooling stage preloads it (`node --test --import ./scripts/root-test-reads-preload.mjs …`). In
// each test file's process it records every tracked repository file outside scripts/ that the test,
// or a Node process it starts, reads. At exit the test file fails if it read a file that
// scripts/root-test-fixtures.json does not declare for it; the planner selects the tooling group for
// every declared file. `node scripts/root-test-fixtures.mjs --write` regenerates the file entries from a
// recorded run, so the dependency is derived from what the tests read, never guessed; reviewed
// directory entries cover tests that index a whole tree.
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';
import { registerHooks, syncBuiltinESMExports } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';

export const MANIFEST_PATH = 'scripts/root-test-fixtures.json';
/** Set by a test file's process for the Node processes it starts: the test that owns their reads. */
export const TEST_ENV = 'HOTSHEET_ROOT_TEST_FILE';
/** The file descendants append their reads to; the owning test file reads it at exit. */
export const LOG_ENV = 'HOTSHEET_ROOT_TEST_READS_LOG';
/** Set by the generator: write each test file's reads to this directory instead of enforcing. */
export const RECORD_ENV = 'HOTSHEET_ROOT_TEST_READS_RECORD';
/** Overrides the repository root (tests of this module only). */
export const ROOT_ENV = 'HOTSHEET_ROOT_TEST_READS_ROOT';
export const REGENERATE = 'node scripts/root-test-fixtures.mjs --write';

const defaultRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

/** The repository-relative POSIX path of `target`, or null for anything that is not a path inside `root`. */
export function repositoryPath(root, target, cwd = process.cwd()) {
  let path;
  if (typeof target === 'string') path = target;
  else if (target instanceof URL) {
    if (target.protocol !== 'file:') return null;
    path = fileURLToPath(target);
  } else if (Buffer.isBuffer(target)) path = target.toString();
  else return null;
  const absolute = isAbsolute(path) ? resolve(path) : resolve(cwd, path);
  const rel = relative(root, absolute);
  if (!rel || rel.startsWith('..') || isAbsolute(rel)) return null;
  return rel.split(sep).join('/');
}

/** Reads that can carry a fixture dependency: outside scripts/ (any change there runs the full gate),
 * Git metadata, installed packages and build output. */
export function fixtureCandidate(path) {
  return !/^(scripts|\.git)\//.test(path) && !/(^|\/)(node_modules|target)\//.test(path);
}

/** The test name for a test file path inside `root`, or null when `file` is not a root script test. */
export function rootTestName(root, file) {
  const path = file ? repositoryPath(root, file) : null;
  const match = path ? /^scripts\/([\w.-]+)\.test\.mjs$/.exec(path) : null;
  return match ? match[1] : null;
}

/**
 * The root tests a manifest names for `path`: its recorded file entry plus every declared directory
 * that contains it. Directories are reviewed declarations for tests that index a whole tree (the local
 * gate's exercise index reads the web client), so a new file there needs no regeneration.
 */
export function fixtureReaders(manifest, path) {
  const readers = new Set(manifest?.fixtures?.[path] ?? []);
  for (const [directory, tests] of Object.entries(manifest?.directories ?? {}))
    if (path.startsWith(directory)) for (const test of tests) readers.add(test);
  return [...readers].sort();
}

/** Reads of `test` that the manifest does not declare for it, sorted. */
export function undeclaredReads(manifest, test, reads) {
  return [...new Set(reads)].filter((path) => !fixtureReaders(manifest, path).includes(test)).sort();
}

/** The manifest from per-test reads: every file a declared directory does not already cover for its
 * reader, sorted, with its sorted readers. The declared directories carry over unchanged. */
export function buildManifest(readsByTest, directories = {}) {
  const declared = { version: 1, directories, fixtures: {} };
  const fixtures = {};
  for (const [test, reads] of Object.entries(readsByTest))
    for (const path of reads) if (!fixtureReaders(declared, path).includes(test)) (fixtures[path] ??= []).push(test);
  const sorted = {};
  for (const path of Object.keys(fixtures).sort()) sorted[path] = [...new Set(fixtures[path])].sort();
  return { version: 1, directories, fixtures: sorted };
}

/** Stable text that Prettier leaves unchanged: one line per directory and file. */
export function formatManifest(manifest) {
  const section = (map) => {
    const entries = Object.entries(map ?? {}).map(
      ([path, tests]) => `    ${JSON.stringify(path)}: [${tests.map((test) => JSON.stringify(test)).join(', ')}]`,
    );
    return entries.length ? `{\n${entries.join(',\n')}\n  }` : '{}';
  };
  return `{\n  "version": ${manifest.version},\n  "directories": ${section(manifest.directories)},\n  "fixtures": ${section(manifest.fixtures)}\n}\n`;
}

export function readManifest(root) {
  try {
    return JSON.parse(fs.readFileSync(join(root, MANIFEST_PATH), 'utf8'));
  } catch {
    return { version: 1, directories: {}, fixtures: {} };
  }
}

/** The subset of `paths` Git tracks in `root`. */
function tracked(root, paths) {
  if (!paths.length) return [];
  const result = spawnSync('git', ['-C', root, 'ls-files', '-z', '--', ...paths], {
    env: withoutGitRepositoryEnv(process.env),
    encoding: 'utf8',
  });
  if (result.status !== 0) return paths;
  const listed = new Set(result.stdout.split('\0').filter(Boolean));
  // A directory read (cpSync) counts as reading each tracked file below it.
  return [...listed].filter((path) => paths.some((read) => path === read || path.startsWith(`${read}/`)));
}

/** Whether `open` flags read the file: the default, an `r` mode, or numeric flags that are not write-only. */
export function opensForReading(flags) {
  if (flags === undefined || flags === null || typeof flags === 'function') return true;
  if (typeof flags === 'number') return (flags & 3) !== 1;
  return typeof flags === 'string' && flags.startsWith('r');
}

/** Patch the fs read entry points to report each path they open for reading. */
export function recordReads(report, target = fs) {
  const wrap = (owner, name, reads = () => true) => {
    const original = owner?.[name];
    if (typeof original !== 'function') return;
    owner[name] = function (...args) {
      if (reads(args)) report(args[0]);
      return original.apply(this, args);
    };
  };
  for (const name of ['readFileSync', 'readFile', 'createReadStream', 'copyFileSync', 'copyFile', 'cpSync', 'cp'])
    wrap(target, name);
  for (const name of ['readFile', 'copyFile', 'cp']) wrap(target.promises, name);
  for (const name of ['openSync', 'open']) wrap(target, name, (args) => opensForReading(args[1]));
  wrap(target.promises, 'open', (args) => opensForReading(args[1]));
  if (target === fs) syncBuiltinESMExports();
}

/** Install the recorder for this process (the preload entry point). */
export function install(options = {}) {
  // The process's own environment by default: descendants inherit the owner and log variables set here.
  const { env: given } = options;
  const env = given === undefined ? process.env : given;
  const argv = options.argv ?? process.argv;
  // Node reports module and working-directory paths physically, so compare against the physical root.
  const root = fs.realpathSync(env[ROOT_ENV] ? resolve(env[ROOT_ENV]) : defaultRoot);
  const owner = env[TEST_ENV] ? null : rootTestName(root, argv[1]);
  if (!owner && !env[TEST_ENV]) return;
  const original = {
    appendFileSync: fs.appendFileSync,
    readFileSync: fs.readFileSync,
    writeFileSync: fs.writeFileSync,
  };
  const reads = new Set();
  const report = (target) => {
    const path = repositoryPath(root, target);
    if (path && fixtureCandidate(path)) reads.add(path);
  };
  recordReads(report);
  // An imported or required module is read too. Modules the preload itself loaded first are cached, so
  // report them here: this recorder's own dependency is every recorded test's dependency.
  registerHooks?.({
    load(url, context, nextLoad) {
      if (url.startsWith('file:')) report(new URL(url));
      return nextLoad(url, context);
    },
  });
  report(new URL('../clients/web/scripts/repository-env.mjs', import.meta.url));
  if (!owner) {
    // A Node process a test started: hand the reads to the owning test file's process.
    const log = env[LOG_ENV];
    process.on('exit', () => {
      if (log && reads.size) original.appendFileSync(log, [...reads].map((path) => `${path}\n`).join(''));
    });
    return;
  }
  const log = join(fs.mkdtempSync(join(tmpdir(), 'hs2-root-test-reads-')), 'reads.log');
  env[TEST_ENV] = owner;
  env[LOG_ENV] = log;
  const preload = `--import=${pathToFileURL(join(dirname(fileURLToPath(import.meta.url)), 'root-test-reads-preload.mjs')).href}`;
  if (!(env.NODE_OPTIONS ?? '').includes(preload)) env.NODE_OPTIONS = `${env.NODE_OPTIONS ?? ''} ${preload}`.trim();
  process.on('exit', () => {
    let logged = [];
    try {
      logged = original.readFileSync(log, 'utf8').split('\n').filter(Boolean);
    } catch {
      // No descendant read anything.
    }
    fs.rmSync(dirname(log), { recursive: true, force: true });
    const all = tracked(root, [...new Set([...reads, ...logged])].sort()).sort();
    if (env[RECORD_ENV]) {
      original.writeFileSync(join(env[RECORD_ENV], `${owner}.json`), `${JSON.stringify(all)}\n`);
      return;
    }
    const missing = undeclaredReads(readManifest(root), owner, all);
    if (!missing.length) return;
    process.stderr.write(
      `HS2-WC74CR: scripts/${owner}.test.mjs read ${missing.length === 1 ? 'a repository file' : 'repository files'} ` +
        `that ${MANIFEST_PATH} does not declare for it, so the local gate would not select it when ` +
        `${missing.length === 1 ? 'that file changes' : 'they change'}:\n${missing.map((path) => `  ${path}\n`).join('')}` +
        `Regenerate the manifest with \`${REGENERATE}\`.\n`,
    );
    process.exitCode = 1;
  });
}
