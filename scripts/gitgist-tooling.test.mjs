// HS2-VA97HW: pinned gitgist commit drafting. Every provider run here uses fake `claude` and
// `codex` executables placed first on PATH, so no real provider, credential or network is used.
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { delimiter, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';
import { destinations } from './sync-ai-playbook.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const json = (path) => JSON.parse(readFileSync(join(root, path), 'utf8'));
const VERSION = '1.2.0';
const INTEGRITY = 'sha512-tPOE/ohDLCEo6blUdfdjNMLTwDD3QdytN5PZC7DIp3xlbqoTPRFHjXjYPPHHVo+gzW3Hn2FjMwvyfucwai4X5Q==';
const installed = join(root, 'node_modules/gitgist/package.json');

async function gitgist() {
  assert.ok(existsSync(installed), 'run `npm ci` at the repository root to install the pinned gitgist');
  return import(join(root, 'node_modules/gitgist/dist/index.js'));
}

/** The pattern/why table in the gitgist section of the canonical local rule. */
function guidanceTable(text) {
  const section = text.slice(text.indexOf('## gitgist commit drafting (HS2-VA97HW)'));
  const body = section.slice(0, section.indexOf('\n## ', 1));
  return [...body.matchAll(/^\| `([^`]+)` \| (.+) \|$/gm)].map(([, pattern, why]) => ({ pattern, why: why.trim() }));
}

// Git's default pathspec wildcard, which gitgist's `:(exclude)` patterns use: `*` also crosses `/`.
const matches = (pattern, path) =>
  new RegExp(
    `^${pattern
      .replace(/[.+^${}()|[\]\\]/g, '\\$&')
      .replaceAll('*', '.*')
      .replaceAll('?', '.')}$`,
  ).test(path);

test('HS2-VA97HW pins gitgist exactly and declares the commit drafting entry points', () => {
  const manifest = json('package.json');
  assert.equal(manifest.private, true);
  assert.equal(manifest.author, 'Small Tale Inc.');
  // Rulesync is the pinned instruction generator checked by check-rulesync-generation.mjs.
  assert.deepEqual(manifest.devDependencies, { gitgist: VERSION, rulesync: '38.0.0' });
  assert.equal(manifest.dependencies, undefined);
  assert.deepEqual(manifest.scripts, {
    'commit:msg': 'gitgist --staged --commit-message',
    'commit:msg:offline': 'gitgist --staged --no-ai',
  });
  assert.equal(manifest.gitgist, undefined, 'settings live only in gitgist.config.json');

  const lock = json('package-lock.json');
  assert.deepEqual(lock.packages[''].devDependencies, { gitgist: VERSION, rulesync: '38.0.0' });
  assert.equal(lock.packages['node_modules/gitgist'].version, VERSION);
  assert.equal(lock.packages['node_modules/gitgist'].integrity, INTEGRITY);
  for (const [path, entry] of Object.entries(lock.packages))
    if (path) assert.equal(entry.hasInstallScript, undefined, `${path} must not run install scripts`);

  assert.ok(existsSync(installed), 'run `npm ci` at the repository root to install the pinned gitgist');
  assert.equal(JSON.parse(readFileSync(installed, 'utf8')).version, VERSION);
});

test('HS2-VA97HW configures only documented keys with signed-in, key-free providers', async () => {
  const { parseConfig, DEFAULT_EXCLUDES } = await gitgist();
  const raw = json('gitgist.config.json');
  const config = parseConfig(raw, 'gitgist.config.json');
  assert.deepEqual(Object.keys(raw).sort(), ['exclude', 'fallbackProvider', 'linkCommits', 'provider']);
  assert.equal(config.provider, 'claude-cli');
  assert.equal(config.fallbackProvider, 'codex');
  assert.equal(config.linkCommits, true);
  for (const pattern of config.exclude)
    assert.ok(!DEFAULT_EXCLUDES.includes(pattern), `${pattern} repeats a built-in gitgist exclusion`);
});

test('HS2-VA97HW documents every exclusion beside its reason and keeps each one live', () => {
  const local = readFileSync(join(root, '.rulesync/rules/project-local.md'), 'utf8');
  const rows = guidanceTable(local);
  const { exclude } = json('gitgist.config.json');
  assert.deepEqual(
    rows.map((row) => row.pattern),
    exclude,
    'the guidance table and gitgist.config.json list the same patterns in the same order',
  );
  for (const row of rows) assert.ok(row.why.length > 10, `${row.pattern} needs a reason`);
  assert.deepEqual(guidanceTable(readFileSync(join(root, 'AGENTS.md'), 'utf8')), rows);

  for (const destination of Object.values(destinations).flat())
    assert.ok(
      exclude.some((pattern) => matches(pattern, destination)),
      `pinned ai-playbook snapshot ${destination} must be excluded`,
    );
  const tracked = execFileSync('git', ['ls-files', '-z'], {
    cwd: root,
    env: withoutGitRepositoryEnv(process.env),
    encoding: 'utf8',
  }).split('\0');
  for (const pattern of exclude)
    assert.ok(
      tracked.some((path) => matches(pattern, path)),
      `${pattern} matches no tracked file; remove it or fix it`,
    );
  for (const source of ['.rulesync/rules/project-local.md', '.claude/skills/hotsheet/SKILL.md', 'gitgist.config.json'])
    assert.ok(!exclude.some((pattern) => matches(pattern, source)), `source ${source} must stay included`);
});

/**
 * An independently initialized repository with this checkout's real entry points and config, the
 * installed gitgist, and fake provider CLIs that record every call.
 */
function synthetic({ claude, codex }) {
  const base = mkdtempSync(join(tmpdir(), 'hs2-va97hw-'));
  const repo = join(base, 'repo');
  const bin = join(base, 'bin');
  const calls = join(base, 'calls.log');
  const prompt = join(base, 'prompt.txt');
  const clean = withoutGitRepositoryEnv(process.env);
  const git = (...args) => execFileSync('git', args, { cwd: repo, env: clean, encoding: 'utf8' });
  const put = (path, text) => {
    mkdirSync(dirname(join(repo, path)), { recursive: true });
    writeFileSync(join(repo, path), text);
  };
  mkdirSync(repo, { recursive: true });
  mkdirSync(bin);
  git('init', '-q', '-b', 'main');
  git('config', 'user.name', 'Hot Sheet Test');
  git('config', 'user.email', 'test@invalid.example');
  put('README.md', 'seed\n');
  git('add', 'README.md');
  git('commit', '-q', '-m', 'Seed');
  for (const file of ['package.json', 'gitgist.config.json']) put(file, readFileSync(join(root, file), 'utf8'));
  symlinkSync(join(root, 'node_modules'), join(repo, 'node_modules'), 'dir');
  put('src/feature.mjs', "export const feature = 'SYNTHETIC_FEATURE_BODY';\n");
  put('AGENTS.md', 'SYNTHETIC_GENERATED_BODY\n');
  put('docs/design/exports/mock.svg', '<svg>SYNTHETIC_EXPORT_BODY</svg>\n');
  git('add', 'src/feature.mjs', 'AGENTS.md', 'docs/design/exports/mock.svg');

  const fake = (name, generate) => {
    const path = join(bin, name);
    writeFileSync(
      path,
      `#!/bin/sh\nprintf '%s %s\\n' ${name} "$1" >> "${calls}"\n[ "$1" = --version ] && exit 0\n${generate}\n`,
    );
    chmodSync(path, 0o755);
  };
  fake('claude', claude);
  fake('codex', codex);
  const env = {
    ...clean,
    PATH: [bin, dirname(process.execPath), process.env.PATH].join(delimiter),
    npm_config_cache: join(base, 'npm-cache'),
    npm_config_update_notifier: 'false',
    ANTHROPIC_API_KEY: '',
    OPENAI_API_KEY: '',
  };
  const npm = (script, ...args) =>
    spawnSync('npm', ['run', '--silent', script, ...(args.length ? ['--', ...args] : [])], {
      cwd: repo,
      env,
      encoding: 'utf8',
    });
  const log = () => (existsSync(calls) ? readFileSync(calls, 'utf8') : '');
  return { npm, log, prompt, dispose: () => rmSync(base, { recursive: true, force: true }) };
}

test('HS2-VA97HW drafts offline through the actual entry point without invoking a provider', async () => {
  await gitgist();
  const repo = synthetic({ claude: 'exit 97', codex: 'exit 98' });
  try {
    const offline = repo.npm('commit:msg:offline');
    assert.equal(offline.status, 0, offline.stderr);
    assert.equal(
      offline.stdout,
      '## Uncommitted changes\n\n### Staged\n- `AGENTS.md`\n- `docs/design/exports/mock.svg`\n- `src/feature.mjs`\n',
    );
    assert.equal(repo.npm('commit:msg:offline').stdout, offline.stdout, 'the offline draft is deterministic');

    const refused = repo.npm('commit:msg', '--no-ai');
    assert.notEqual(refused.status, 0);
    assert.match(refused.stderr, /--format commit requires AI; remove --no-ai\./);
    assert.equal(repo.log(), '', 'no provider CLI was probed or run');
  } finally {
    repo.dispose();
  }
});

test('HS2-VA97HW drafts with signed-in claude, retries codex, and withholds excluded diff bodies', async () => {
  await gitgist();
  const repo = synthetic({
    claude: 'echo "not signed in" >&2\nexit 9',
    codex: `cat > "${'$'}{0%/bin/codex}/prompt.txt"\necho 'HS2-TEST Add the synthetic feature'`,
  });
  try {
    const draft = repo.npm('commit:msg');
    assert.equal(draft.status, 0, draft.stderr);
    assert.equal(draft.stdout, 'HS2-TEST Add the synthetic feature\n');
    assert.match(draft.stderr, /primary provider failed \(claude exited with code 9: not signed in\)/);
    assert.deepEqual(repo.log().trim().split('\n'), [
      'claude --version',
      'claude --append-system-prompt',
      'codex --version',
      'codex exec',
    ]);

    const prompt = readFileSync(repo.prompt, 'utf8');
    assert.match(prompt, /SYNTHETIC_FEATURE_BODY/);
    assert.match(prompt, /AGENTS\.md/, 'an excluded file is still named as changed');
    assert.doesNotMatch(prompt, /SYNTHETIC_GENERATED_BODY|SYNTHETIC_EXPORT_BODY/);
  } finally {
    repo.dispose();
  }
});
