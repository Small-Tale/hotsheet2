// HS2-KKDZF4: Node scripts and test fixtures never inherit a hook's repository environment.
//
// Under a git hook (`pre-push` runs `scripts/check.sh`), git exports `GIT_DIR`, `GIT_PREFIX`, and
// for `pre-commit` `GIT_INDEX_FILE`. A fixture's `git -C <tmp> commit` started with them inherited
// commits into the *calling* repository and runs its hooks, and `git init --bare <remote>`
// re-initializes the calling repository with `core.bare = true`. This file mirrors the Rust
// `crates/hotsheet-cli/tests/git_env_isolation.rs` guard for the Node side: a source scan pins
// every git spawn to `withoutGitRepositoryEnv`, and the real fixtures run against a sentinel
// repository that must stay byte-identical.
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative, resolve } from 'node:path';
import test from 'node:test';

import { removeGitRepositoryEnv, withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';

const root = resolve(import.meta.dirname, '..');
const SCANNED = ['scripts', 'clients/web/scripts', 'clients/web/tests', 'migrator'];
const SKIPPED_DIRECTORIES = new Set([
  'node_modules',
  'target',
  'dist',
  'fixtures',
  'test-results',
  'playwright-report',
]);
const SPAWNERS = ['spawn', 'spawnSync', 'execFile', 'execFileSync', 'exec', 'execSync'];

function sources() {
  const out = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) {
        if (!SKIPPED_DIRECTORIES.has(entry.name)) walk(path);
      } else if (/\.(mjs|cjs|js|ts)$/.test(entry.name)) out.push(path);
    }
  };
  for (const dir of SCANNED) walk(join(root, dir));
  return out;
}

/** The index of the quote closing the string or template that opens at `start`. */
function stringEnd(text, start) {
  const quote = text[start];
  let index = start + 1;
  while (index < text.length && text[index] !== quote) {
    if (text[index] === '\\') index += 2;
    else if (quote === '`' && text[index] === '$' && text[index + 1] === '{') index = balancedEnd(text, index + 1) + 1;
    else index += 1;
  }
  return index;
}

/** The index of the bracket matching the one at `open`, skipping strings, templates, and comments. */
function balancedEnd(text, open) {
  let depth = 0;
  for (let index = open; index < text.length; index += 1) {
    const char = text[index];
    if (char === '/' && text[index + 1] === '/') index = text.indexOf('\n', index);
    else if (char === '/' && text[index + 1] === '*') index = text.indexOf('*/', index) + 1;
    else if (char === "'" || char === '"' || char === '`') index = stringEnd(text, index);
    else if (char === '(' || char === '[' || char === '{') depth += 1;
    else if (char === ')' || char === ']' || char === '}') {
      depth -= 1;
      if (depth === 0) return index;
    }
    if (index < 0) break;
  }
  throw new Error(`Unbalanced call at offset ${open}`);
}

const callArguments = (text, open) => text.slice(open + 1, balancedEnd(text, open));

/** Every reason `text` could hand a child process an inherited repository environment. */
export function gitEnvironmentViolations(text) {
  if (!/['"]node:child_process['"]|['"]child_process['"]/.test(text)) return [];
  if (!/[(,]\s*['"]git['"]\s*[,)]/.test(text)) return [];
  const violations = [];
  if (!text.includes('withoutGitRepositoryEnv')) violations.push('does not use withoutGitRepositoryEnv');
  for (const match of text.matchAll(/(\.\.\.|\benv\s*[:=]\s*|\?\?\s*)process\.env\b(?!\s*[.[])/g))
    violations.push(`passes the inherited process.env (${match[0].trim()})`);
  const aliases = [...text.matchAll(/\b(\w+)\s*=\s*promisify\(\s*(?:execFile|exec)\s*\)/g)].map((match) => match[1]);
  const names = [...SPAWNERS, ...aliases].join('|');
  for (const match of text.matchAll(new RegExp(`(?<![.\\w])(${names})\\s*\\(`, 'g'))) {
    const args = callArguments(text, match.index + match[0].length - 1);
    const program = /^\s*(['"`])([^'"`]*)\1/.exec(args)?.[2];
    if (program !== undefined && program !== 'git') continue;
    if (!/\benv\b/.test(args)) {
      const line = text.slice(0, match.index).split('\n').length;
      violations.push(
        `line ${line}: ${match[1]}(${program === undefined ? args.split(',')[0].trim() : "'git'"}, …) without env`,
      );
    }
  }
  return violations;
}

test('HS2-KKDZF4 every Node git spawn uses the scrubbed repository environment', () => {
  const self = resolve(import.meta.filename);
  const failures = sources()
    .filter((path) => path !== self)
    .flatMap((path) =>
      gitEnvironmentViolations(readFileSync(path, 'utf8')).map((reason) => `${relative(root, path)}: ${reason}`),
    );
  assert.deepEqual(failures, []);
});

test('HS2-KKDZF4 the scan rejects each way a fixture can inherit the hook environment', () => {
  const header = "import { execFileSync, spawnSync } from 'node:child_process';\n";
  const clean = `${header}import { withoutGitRepositoryEnv } from './repository-env.mjs';\n`;
  assert.deepEqual(gitEnvironmentViolations(`${header}execFileSync('git', ['status'], { env: {} });`), [
    'does not use withoutGitRepositoryEnv',
  ]);
  assert.match(gitEnvironmentViolations(`${clean}execFileSync('git', ['init'], { cwd });`)[0], /'git'.*without env/);
  assert.match(
    gitEnvironmentViolations(`${clean}const run = (p, a) => spawnSync(p, a, { cwd });\nrun('git', []);`)[0],
    /line 3: spawnSync\(p, …\) without env/,
  );
  assert.match(
    gitEnvironmentViolations(`${clean}spawnSync('git', [], { env: { ...process.env, A: '1' } });`)[0],
    /inherited process\.env/,
  );
  assert.match(gitEnvironmentViolations(`${clean}const run = (env = process.env) => run('git');`)[0], /inherited/);
  assert.deepEqual(
    gitEnvironmentViolations(
      `${clean}const env = { ...withoutGitRepositoryEnv(process.env), A: process.env.A };\n` +
        "spawnSync('git', ['log', `${a}(`], { env });\nspawnSync('cargo', ['metadata']);\n/x/.exec('(');",
    ),
    [],
  );
});

test('HS2-KKDZF4 the local gate scrubs the repository environment before any stage', () => {
  const gate = readFileSync(join(root, 'scripts/local-gate.mjs'), 'utf8');
  const entry = gate.indexOf('if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {');
  assert.ok(entry > 0);
  const scrub = gate.indexOf('removeGitRepositoryEnv(process.env)', entry);
  assert.ok(scrub > entry && scrub < gate.indexOf('runGate(', entry), 'scrub precedes runGate');
  const env = { GIT_DIR: '/hook', GIT_INDEX_FILE: '/hook/index', GIT_AUTHOR_NAME: 'kept' };
  assert.deepEqual(removeGitRepositoryEnv(env), ['GIT_DIR', 'GIT_INDEX_FILE']);
  assert.deepEqual(env, { GIT_AUTHOR_NAME: 'kept' });
});

const quiet = { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] };
const gitIn = (dir, ...args) =>
  execFileSync('git', ['-C', dir, ...args], { ...quiet, env: withoutGitRepositoryEnv(process.env) });

/** Every file under `dir` (work tree and `.git`) with its bytes. */
function snapshot(dir) {
  const out = {};
  const walk = (current) => {
    for (const name of readdirSync(current)) {
      const path = join(current, name);
      if (statSync(path).isDirectory()) walk(path);
      else out[relative(dir, path)] = readFileSync(path).toString('base64');
    }
  };
  walk(dir);
  return out;
}

test(
  'HS2-KKDZF4 real fixtures under a hook environment leave the sentinel repository byte-identical',
  { timeout: 180_000 },
  () => {
    const scratch = mkdtempSync(join(tmpdir(), 'hs2-kkdzf4-'));
    try {
      const sentinel = join(scratch, 'sentinel');
      gitIn(scratch, 'init', '-q', '-b', 'main', sentinel);
      for (const [key, value] of [
        ['user.name', 'Sentinel'],
        ['user.email', 'sentinel@example.invalid'],
        ['maintenance.auto', 'false'],
        ['gc.auto', '0'],
        ['core.hooksPath', join(scratch, 'no-hooks')],
      ])
        gitIn(sentinel, 'config', key, value);
      writeFileSync(join(sentinel, 'README'), 'sentinel\n');
      gitIn(sentinel, 'add', 'README');
      gitIn(sentinel, 'commit', '-q', '-m', 'sentinel');
      // HS2-RY0ZMX: a leaked environment would let window fixtures rewrite this shared ref.
      gitIn(sentinel, 'update-ref', 'refs/hotsheet/main-integration-window', 'HEAD');
      gitIn(sentinel, 'status', '--porcelain');
      const before = snapshot(sentinel);
      const gitDir = join(sentinel, '.git');
      // What a linked-worktree `pre-push` exports, plus `pre-commit`'s index.
      const hostile = {
        ...withoutGitRepositoryEnv(process.env),
        GIT_DIR: gitDir,
        GIT_PREFIX: '',
        GIT_INDEX_FILE: join(gitDir, 'index'),
      };
      // A nested runner must report TAP to this test, not serialize to this runner's parent.
      delete hostile.NODE_TEST_CONTEXT;
      const fixtures = [
        ['scripts/sync-ai-playbook.test.mjs', 'real git source produces an offline bundle'],
        ['scripts/main-integration-window.test.mjs', 'HS2-BND42N expired owner is replaced and its ref is released'],
        ['scripts/git-hooks.test.mjs', '.'],
      ];
      for (const [file, pattern] of fixtures) {
        const result = spawnSync(
          process.execPath,
          ['--test', '--test-reporter=tap', `--test-name-pattern=${pattern}`, file],
          {
            cwd: root,
            env: hostile,
            encoding: 'utf8',
            timeout: 120_000,
          },
        );
        assert.equal(result.status, 0, `${file} under a hook environment:\n${result.stdout}${result.stderr}`);
        assert.match(result.stdout, /# pass [1-9]/, `${file} ran no test`);
        assert.deepEqual(snapshot(sentinel), before, `${file} changed the sentinel repository`);
      }
      assert.match(readFileSync(join(gitDir, 'config'), 'utf8'), /bare = false/);
    } finally {
      rmSync(scratch, { recursive: true, force: true });
    }
  },
);
