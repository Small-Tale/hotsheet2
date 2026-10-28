import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const source = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const run = (program, args, cwd, env = process.env) =>
  execFileSync(program, args, { cwd, env, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
const put = (path, content) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content);
};
const executable = (path, content) => {
  put(path, content);
  chmodSync(path, 0o755);
};

function fixture() {
  const root = mkdtempSync(resolve(tmpdir(), 'hs-git-hooks-'));
  const repo = resolve(root, 'repo');
  mkdirSync(repo);
  run('git', ['init', '-b', 'main'], repo);
  run('git', ['config', 'user.name', 'Hot Sheet Test'], repo);
  run('git', ['config', 'user.email', 'test@invalid.example'], repo);
  for (const path of ['clients/web', 'migrator', 'spikes/kerf-webawesome'])
    mkdirSync(resolve(repo, path), { recursive: true });
  for (const path of ['scripts/check.sh', 'scripts/push.sh', '.githooks/pre-commit', '.githooks/pre-push']) {
    const target = resolve(repo, path);
    mkdirSync(dirname(target), { recursive: true });
    copyFileSync(resolve(source, path), target);
    chmodSync(target, 0o755);
  }
  put(resolve(repo, 'seed'), 'seed\n');
  run(
    'git',
    ['add', 'seed', 'scripts/check.sh', 'scripts/push.sh', '.githooks/pre-commit', '.githooks/pre-push'],
    repo,
  );
  run('git', ['commit', '-m', 'Seed'], repo);
  return { root, repo, dispose: () => rmSync(root, { recursive: true, force: true }) };
}

test('HS2-RS0FFT required gate uses receipt validation without an environment bypass', () => {
  assert.match(readFileSync(resolve(source, 'scripts/check.sh'), 'utf8'), /local-gate\.mjs/);
  assert.doesNotMatch(readFileSync(resolve(source, '.githooks/pre-push'), 'utf8'), /HOTSHEET_GATE_PASSED_TREE/);
});

test('HS2-CD3D2T push gates before remote contact and pre-push skips that same tree', () => {
  const f = fixture();
  try {
    const log = resolve(f.root, 'gate.log');
    const fail = resolve(f.root, 'fail');
    const bin = resolve(f.root, 'bin');
    executable(resolve(bin, 'zsh'), '#!/bin/sh\nexit 0\n');
    executable(
      resolve(f.repo, 'scripts/check.sh'),
      `#!/bin/sh\ntree=$(git rev-parse 'HEAD^{tree}')\n[ -f ".git/test-gate-$tree" ] && exit 0\nprintf 'check\\n' >> "$HOTSHEET_HOOK_TEST_LOG"\n[ ! -f "$HOTSHEET_HOOK_TEST_FAIL" ] || exit 1\ntouch ".git/test-gate-$tree"\n`,
    );
    const remote = resolve(f.root, 'remote.git');
    run('git', ['init', '--bare', remote], f.root);
    run('git', ['remote', 'add', 'origin', remote], f.repo);
    run('git', ['config', 'core.hooksPath', '.githooks'], f.repo);
    const env = {
      ...process.env,
      PATH: `${bin}:${process.env.PATH}`,
      HOTSHEET_HOOK_TEST_LOG: log,
      HOTSHEET_HOOK_TEST_FAIL: fail,
      HOTSHEET_GATE_PASSED_TREE: run('git', ['rev-parse', 'HEAD^{tree}'], f.repo).trim(),
    };
    run('bash', ['scripts/push.sh'], f.repo, env);
    assert.equal(readFileSync(log, 'utf8'), 'check\n');
    const pushed = run('git', ['rev-parse', 'refs/heads/main'], remote).trim();
    put(resolve(f.repo, 'next'), 'next\n');
    run('git', ['add', 'next'], f.repo);
    run('git', ['commit', '-m', 'Next'], f.repo, env);
    put(fail, 'fail');
    assert.throws(() => run('bash', ['scripts/push.sh'], f.repo, env));
    assert.equal(run('git', ['rev-parse', 'refs/heads/main'], remote).trim(), pushed);
    rmSync(fail);
    run('git', ['push', 'origin', 'main'], f.repo, env);
    assert.equal(readFileSync(log, 'utf8'), 'check\ncheck\ncheck\n');
    assert.notEqual(run('git', ['rev-parse', 'refs/heads/main'], remote).trim(), pushed);
  } finally {
    f.dispose();
  }
});

test('HS2-CD3D2T pre-commit rejects staged whitespace before formatting', () => {
  const f = fixture();
  try {
    const bin = resolve(f.root, 'bin');
    const log = resolve(f.root, 'format.log');
    executable(resolve(bin, 'zsh'), '#!/bin/sh\nprintf "format\\n" >> "$HOTSHEET_HOOK_TEST_LOG"\n');
    run('git', ['config', 'core.hooksPath', '.githooks'], f.repo);
    const env = { ...process.env, PATH: `${bin}:${process.env.PATH}`, HOTSHEET_HOOK_TEST_LOG: log };
    put(resolve(f.repo, 'bad'), 'bad  \n');
    run('git', ['add', 'bad'], f.repo);
    assert.throws(() => run('git', ['commit', '-m', 'Bad'], f.repo, env));
    assert.equal(existsSync(log), false);
    put(resolve(f.repo, 'bad'), 'good\n');
    run('git', ['add', 'bad'], f.repo);
    run('git', ['commit', '-m', 'Good'], f.repo, env);
    assert.equal(readFileSync(log, 'utf8'), 'format\n');
  } finally {
    f.dispose();
  }
});
