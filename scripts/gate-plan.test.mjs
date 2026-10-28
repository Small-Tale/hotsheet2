// HS2-FCCRCK: the shadow gate planner. Synthetic Git repositories only; nothing here runs the real gate.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

import { classifyPath, collect, execute, main, parseCheckScript, plan, stageGroup } from './gate-plan.mjs';

const realCheck = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'check-full.sh'), 'utf8');
const stages = parseCheckScript(realCheck);
const selectedGroups = (changes) => {
  const result = plan({ stages, changes });
  return {
    mode: result.mode,
    groups: [...new Set(result.stages.filter((s) => s.selected).map((s) => s.group))].sort(),
  };
};

test('HS2-FCCRCK reads every stage of the real scripts/check.sh and classifies each one', () => {
  assert.equal(stages.length, 16);
  assert.deepEqual(
    stages.filter((stage) => stageGroup(stage) === undefined),
    [],
    'a new or renamed gate stage must be classified before the planner can omit anything',
  );
  assert.deepEqual([...new Set(stages.map(stageGroup))].sort(), ['always', 'migrator', 'rust', 'spike', 'web']);
});

test('HS2-FCCRCK an unclassified gate stage forces the full gate', () => {
  const extended = parseCheckScript(`${realCheck}\ncargo deny check\n`);
  const result = plan({ stages: extended, changes: ['clients/web/src/app.ts'] });
  assert.equal(result.mode, 'full');
  assert.match(result.fallback.join('\n'), /unclassified gate stage: \. cargo deny check/);
  assert.throws(() => parseCheckScript('(\n  cd clients/web\n  npm test\n)\n'), /Unrecognized gate subshell/);
  assert.throws(() => parseCheckScript(`${realCheck}\npython3 unexpected.py\n`), /Unrecognized gate statement/);
});

test('HS2-FCCRCK selects each package and its dependents from source-derived edges', () => {
  const always = ['always'];
  assert.deepEqual(selectedGroups(['clients/web/src/app.ts']), { mode: 'selected', groups: [...always, 'web'].sort() });
  assert.deepEqual(selectedGroups(['docs/06-clients.md']), { mode: 'selected', groups: ['always', 'web'] });
  assert.deepEqual(selectedGroups(['crates/hotsheet-cli/src/lib.rs']), {
    mode: 'selected',
    groups: ['always', 'migrator', 'rust', 'web'],
  });
  assert.deepEqual(selectedGroups(['spikes/kerf-webawesome/src/main.ts']), {
    mode: 'selected',
    groups: ['always', 'spike', 'web'],
  });
  assert.deepEqual(selectedGroups(['migrator/src/export.mjs']), {
    mode: 'selected',
    groups: ['always', 'migrator', 'rust', 'web'],
  });
  // Cross-package edges the path prefix alone would miss.
  assert.deepEqual(selectedGroups(['clients/web/public/favicon-32.png']).groups, ['always', 'rust', 'web']);
  assert.deepEqual(selectedGroups(['plugins/shared/visual-qa.md']).groups, ['always', 'rust']);
  assert.deepEqual(selectedGroups(['.claude/skills/hotsheet/SKILL.md']).groups, ['always', 'rust']);
  assert.deepEqual(selectedGroups(['docs/contracts/api.json']).groups, ['always', 'web']);
});

test('HS2-FCCRCK falls back to the full gate for gate, config, lock, toolchain, guidance and unknown paths', () => {
  for (const path of [
    'scripts/check.sh',
    'scripts/gate-plan.mjs',
    '.githooks/pre-push',
    '.github/workflows/ci.yml',
    'Cargo.lock',
    'Cargo.toml',
    'rust-toolchain.toml',
    '.cargo/config.toml',
    'clients/web/package.json',
    'clients/web/package-lock.json',
    'migrator/package-lock.json',
    'package.json',
    'AGENTS.md',
    '.rulesync/rules/project-local.md',
    'guidance/ai-playbook.lock.json',
    'clients/web/src-tauri/src/main.rs',
    '.gitignore',
    'Makefile',
  ]) {
    const { groups, reason } = classifyPath(path);
    assert.equal(groups, 'full', path);
    const result = plan({ stages, changes: ['clients/web/src/app.ts', path] });
    assert.equal(result.mode, 'full', path);
    assert.ok(
      result.stages.every((stage) => stage.selected),
      path,
    );
    assert.ok(
      result.fallback.some((line) => line.startsWith(`${path}: ${reason}`)),
      path,
    );
  }
  assert.equal(plan({ stages, changes: [] }).mode, 'full', 'no changes cannot justify omitting anything');
  assert.equal(plan({ stages, changes: ['crates/x.rs'], fallback: ['base cannot be resolved'] }).mode, 'full');
});

test('HS2-FCCRCK lists every omitted stage with its reason and keeps repository-wide checks', () => {
  const result = plan({ stages, changes: ['clients/web/src/app.ts'] });
  const omitted = result.stages.filter((stage) => !stage.selected);
  assert.deepEqual(
    omitted.map((stage) => stage.command),
    [
      'cargo lint',
      'cargo nextest run --all-features --status-level leak',
      'npm run lint',
      'npm run test:coverage',
      'npm run lint',
      'npm run typecheck',
      'npm run build',
    ],
  );
  assert.ok(omitted.every((stage) => /^no changed path reaches the \w+ group$/.test(stage.reasons[0])));
  for (const command of ['cargo fmt --all --check', 'node scripts/check-test-coverage.mjs', 'npm run lint'])
    assert.ok(result.stages.some((stage) => stage.command === command && stage.selected));
});

function repository(t) {
  const repo = mkdtempSync(join(tmpdir(), 'hs2-fccrck-plan-'));
  t.after(() => rmSync(repo, { recursive: true, force: true }));
  const git = (...args) => execFileSync('git', ['-C', repo, ...args], { encoding: 'utf8' }).trim();
  git('init', '--quiet', '-b', 'main');
  git('config', 'user.email', 'synthetic@example.invalid');
  git('config', 'user.name', 'Synthetic');
  const write = (path, text) => {
    mkdirSync(dirname(join(repo, path)), { recursive: true });
    writeFileSync(join(repo, path), text);
  };
  write('scripts/check.sh', realCheck);
  write('scripts/check-full.sh', realCheck);
  write('clients/web/src/app.ts', 'export const app = 1;\n');
  write('crates/hotsheet-cli/src/lib.rs', 'pub fn cli() {}\n');
  git('add', '.');
  git('commit', '--quiet', '-m', 'base');
  git('update-ref', 'refs/remotes/origin/main', 'HEAD');
  return { repo, git, write };
}

test('HS2-FCCRCK collects committed, staged, unstaged, deleted and untracked changes and marks a dirty tree', (t) => {
  const { repo, git, write } = repository(t);
  write('clients/web/src/app.ts', 'export const app = 2;\n');
  git('commit', '--quiet', '-am', 'committed web change');
  const clean = collect(repo);
  assert.deepEqual(clean.changes, ['clients/web/src/app.ts']);
  assert.equal(clean.dirty, false);
  write('docs/new.md', 'untracked\n');
  git('rm', '--quiet', 'crates/hotsheet-cli/src/lib.rs');
  const dirty = collect(repo);
  assert.deepEqual(dirty.changes, ['clients/web/src/app.ts', 'crates/hotsheet-cli/src/lib.rs', 'docs/new.md']);
  assert.equal(dirty.dirty, true);
  const logs = [];
  main(['--json'], { repo, log: (line) => logs.push(line) });
  const report = JSON.parse(logs.join('\n'));
  assert.equal(report.verifiedTree, null, 'a dirty plan is never labelled as the verified HEAD tree');
  assert.ok(
    report.stages.find((stage) => stage.command === 'cargo lint').selected,
    'the deleted Rust file selects Rust',
  );
});

test('HS2-FCCRCK uses the merge base, and an unknown or unrelated base forces the full gate', (t) => {
  const { repo, git, write } = repository(t);
  const base = git('rev-parse', 'HEAD');
  git('checkout', '--quiet', '-b', 'feature');
  write('clients/web/src/app.ts', 'export const app = 3;\n');
  git('commit', '--quiet', '-am', 'feature change');
  git('checkout', '--quiet', 'main');
  write('crates/hotsheet-cli/src/lib.rs', 'pub fn moved() {}\n');
  git('commit', '--quiet', '-am', 'main moved on');
  git('update-ref', 'refs/remotes/origin/main', 'HEAD');
  git('checkout', '--quiet', 'feature');
  const state = collect(repo);
  assert.equal(state.base, base, 'the plan is relative to the merge base, not the moved main');
  assert.deepEqual(state.changes, ['clients/web/src/app.ts']);
  const missing = collect(repo, 'refs/remotes/origin/absent');
  assert.equal(missing.base, null);
  assert.equal(plan({ stages, changes: missing.changes, fallback: missing.fallback }).mode, 'full');
  const verified = [];
  main(['--json'], { repo, log: (line) => verified.push(line) });
  assert.equal(JSON.parse(verified.join('\n')).verifiedTree, git('rev-parse', 'HEAD^{tree}'));
});

test('HS2-FCCRCK execution runs selected stages in order and propagates the first failure or signal', () => {
  const selected = [
    { cwd: '.', command: 'first' },
    { cwd: '.', command: 'second' },
    { cwd: '.', command: 'third' },
  ];
  const seen = [];
  const failing = execute(selected, {
    runner: (stage) => {
      seen.push(stage.command);
      return { status: stage.command === 'second' ? 3 : 0 };
    },
  });
  assert.equal(failing.status, 3);
  assert.deepEqual(seen, ['first', 'second'], 'no stage runs after a failure');
  const interrupted = execute(selected, { runner: () => ({ status: null, signal: 'SIGINT' }) });
  assert.equal(interrupted.status, 128);
  assert.equal(interrupted.results.length, 1);
  assert.equal(execute(selected, { runner: () => ({ status: 0 }) }).status, 0);
});

test('HS2-FCCRCK execute mode never touches the root-gate tree cache', (t) => {
  const { repo, git, write } = repository(t);
  write('clients/web/src/app.ts', 'export const app = 4;\n');
  git('commit', '--quiet', '-am', 'web change');
  const commands = [];
  const status = main(['--execute'], {
    repo,
    log: () => {},
    runner: (stage) => (commands.push(stage.command), { status: 0 }),
  });
  assert.equal(status, 0);
  assert.ok(!commands.includes('cargo lint'));
  assert.throws(() => git('rev-parse', '--verify', '--quiet', 'refs/hotsheet-root-gate'));
  const cache = execFileSync('git', ['-C', repo, 'rev-parse', '--git-path', 'hotsheet-root-gate'], {
    encoding: 'utf8',
  }).trim();
  assert.throws(() => readFileSync(join(repo, cache, git('rev-parse', 'HEAD^{tree}'))));
});
