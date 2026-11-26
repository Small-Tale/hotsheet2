#!/usr/bin/env node
// HS2-38CA53: open the docs/architecture Structurizr workspace in the local
// viewer with one command (`npm run view-architecture-docs`).
//
// 1. Finds a JDK 21+ (JAVA_HOME, Homebrew's keg-only openjdk, or `java` on PATH).
// 2. Builds structurizr.war from the pinned release tag once and caches it under
//    ~/.cache/hotsheet2/structurizr/<tag> (override with HOTSHEET_STRUCTURIZR_CACHE).
// 3. Validates the workspace, copies docs/architecture to a scratch directory so
//    viewer-generated files stay out of Git, and runs `structurizr.war local`.
//
// Flags: --no-validate, --validate-only, --rebuild. Environment: PORT (default 8080).

import { spawnSync, spawn } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, statSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';

export const STRUCTURIZR_TAG = 'v2026.10.26';
export const STRUCTURIZR_REPO = 'https://github.com/structurizr/structurizr.git';
export const MIN_JAVA = 21;

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const WORKSPACE_DIR = join(ROOT, 'docs', 'architecture');

/** Major version from `java -version` output (`openjdk version "27.0.1" ...`, or legacy `"1.8.0"`). */
export function javaMajor(versionOutput) {
  const match = /version "(\d+)(?:\.(\d+))?/.exec(versionOutput);
  if (!match) return null;
  const major = Number(match[1]);
  return major === 1 && match[2] ? Number(match[2]) : major;
}

/** Candidate java executables in preference order; duplicates removed. */
export function javaCandidates(env, exists) {
  const list = [];
  if (env.JAVA_HOME) list.push(join(env.JAVA_HOME, 'bin', 'java'));
  for (const prefix of ['/opt/homebrew/opt', '/usr/local/opt']) {
    for (const name of ['openjdk', 'openjdk@27', 'openjdk@25', 'openjdk@21']) {
      list.push(join(prefix, name, 'bin', 'java'));
    }
  }
  return [...new Set(list.filter((path) => exists(path))), 'java'];
}

/** The largest .war under `dir` (the executable application archive), or null. */
export function findWar(dir, minBytes = 10 * 1024 * 1024) {
  let best = null;
  const walk = (current) => {
    for (const name of readdirSync(current)) {
      if (name === '.git' || name === 'node_modules') continue;
      const full = join(current, name);
      const st = statSync(full);
      if (st.isDirectory()) walk(full);
      else if (name.endsWith('.war') && st.size >= minBytes && (!best || st.size > best.size)) {
        best = { path: full, size: st.size };
      }
    }
  };
  if (existsSync(dir)) walk(dir);
  return best?.path ?? null;
}

function fail(message) {
  console.error(`view-architecture-docs: ${message}`);
  process.exit(1);
}

function resolveJava() {
  for (const candidate of javaCandidates(process.env, existsSync)) {
    const probe = spawnSync(candidate, ['-version'], { encoding: 'utf8', env: withoutGitRepositoryEnv(process.env) });
    if (probe.status !== 0) continue;
    const major = javaMajor(`${probe.stderr}${probe.stdout}`);
    if (major && major >= MIN_JAVA) return { java: candidate, home: join(candidate, '..', '..'), major };
  }
  return fail(`no JDK ${MIN_JAVA}+ found. Install one (brew install openjdk) or set JAVA_HOME.`);
}

function run(command, args, options) {
  const result = spawnSync(command, args, { stdio: 'inherit', env: withoutGitRepositoryEnv(process.env), ...options });
  if (result.status !== 0) fail(`${command} ${args.join(' ')} exited with ${result.status ?? result.signal}`);
}

function ensureWar(javaHome, rebuild) {
  const cache =
    process.env.HOTSHEET_STRUCTURIZR_CACHE ?? join(homedir(), '.cache', 'hotsheet2', 'structurizr', STRUCTURIZR_TAG);
  const source = join(cache, 'src');
  if (rebuild) rmSync(cache, { recursive: true, force: true });
  const cached = findWar(source);
  if (cached) return cached;
  mkdirSync(cache, { recursive: true });
  if (!existsSync(join(source, 'mvnw'))) {
    rmSync(source, { recursive: true, force: true });
    console.log(`Cloning Structurizr ${STRUCTURIZR_TAG} into ${source} ...`);
    run('git', [
      '-c',
      'advice.detachedHead=false',
      'clone',
      '--quiet',
      '--depth',
      '1',
      '--branch',
      STRUCTURIZR_TAG,
      STRUCTURIZR_REPO,
      source,
    ]);
  }
  console.log('Building structurizr.war (first run only; several minutes) ...');
  run('./mvnw', ['-q', '-DskipTests', 'package'], {
    cwd: source,
    env: { ...withoutGitRepositoryEnv(process.env), JAVA_HOME: javaHome },
  });
  return findWar(source) ?? fail(`the build produced no .war under ${source}`);
}

function main(argv) {
  const { java, home, major } = resolveJava();
  // JDK 26+ warns about Jackson's reflective final-field writes (JEP 500); allow them quietly.
  const jvm = major >= 26 ? ['--enable-final-field-mutation=ALL-UNNAMED'] : [];
  const war = ensureWar(home, argv.includes('--rebuild'));
  const workspace = join(WORKSPACE_DIR, 'workspace.dsl');

  if (!argv.includes('--no-validate')) {
    console.log('Validating docs/architecture/workspace.dsl ...');
    run(java, [...jvm, '-jar', war, 'validate', '-workspace', workspace]);
  }
  if (argv.includes('--validate-only')) return;

  const scratch = mkdtempSync(join(tmpdir(), 'hs2-architecture-'));
  cpSync(WORKSPACE_DIR, scratch, { recursive: true });
  const port = process.env.PORT ?? '8080';
  console.log(`Serving a copy in ${scratch} at http://localhost:${port} (Ctrl-C to stop).`);
  console.log('Edits to docs/architecture need a restart of this command to show up.');
  const child = spawn(java, [...jvm, `-Dserver.port=${port}`, '-jar', war, 'local', scratch], {
    stdio: 'inherit',
    env: withoutGitRepositoryEnv(process.env),
  });
  const stop = () => child.kill('SIGTERM');
  process.on('SIGINT', stop);
  process.on('SIGTERM', stop);
  child.on('exit', (code) => {
    rmSync(scratch, { recursive: true, force: true });
    process.exit(code ?? 0);
  });
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main(process.argv.slice(2));
