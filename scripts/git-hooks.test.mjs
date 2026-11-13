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
import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';

const source = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const run = (program, args, cwd, env = withoutGitRepositoryEnv(process.env)) =>
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
  for (const path of [
    'scripts/check.sh',
    'scripts/push.sh',
    'scripts/main-integration-window.mjs',
    'scripts/main-integration-options.mjs',
    'clients/web/scripts/repository-env.mjs',
    '.githooks/pre-commit',
    '.githooks/pre-push',
  ]) {
    const target = resolve(repo, path);
    mkdirSync(dirname(target), { recursive: true });
    copyFileSync(resolve(source, path), target);
    chmodSync(target, 0o755);
  }
  put(resolve(repo, 'seed'), 'seed\n');
  run(
    'git',
    [
      'add',
      'seed',
      'scripts/check.sh',
      'scripts/push.sh',
      'scripts/main-integration-window.mjs',
      'scripts/main-integration-options.mjs',
      'clients/web/scripts/repository-env.mjs',
      '.githooks/pre-commit',
      '.githooks/pre-push',
    ],
    repo,
  );
  run('git', ['commit', '-m', 'Seed'], repo);
  return { root, repo, dispose: () => rmSync(root, { recursive: true, force: true }) };
}

test('HS2-RS0FFT required gate uses receipt validation without an environment bypass', () => {
  assert.match(readFileSync(resolve(source, 'scripts/check.sh'), 'utf8'), /local-gate\.mjs/);
  assert.doesNotMatch(readFileSync(resolve(source, '.githooks/pre-push'), 'utf8'), /HOTSHEET_GATE_PASSED_TREE/);
});

test('HS2-BND42N reserves main before gate and rejects direct main push', () => {
  const f = fixture();
  try {
    const log = resolve(f.root, 'gate.log');
    const fail = resolve(f.root, 'fail');
    const bin = resolve(f.root, 'bin');
    executable(
      resolve(bin, 'zsh'),
      '#!/bin/sh\nif [ "$2" = "npm run format:check" ]; then exit 0; fi\nexec /bin/sh "$@"\n',
    );
    executable(
      resolve(f.repo, 'scripts/check.sh'),
      `#!/bin/sh\ntree=$(git rev-parse 'HEAD^{tree}')\nif [ "\${1:-}" = '--verify-receipt' ]; then test -f ".git/test-gate-$tree"; exit; fi\n[ -f ".git/test-gate-$tree" ] && exit 0\nprintf 'check\\n' >> "$HOTSHEET_HOOK_TEST_LOG"\n[ ! -f "$HOTSHEET_HOOK_TEST_FAIL" ] || exit 1\ntouch ".git/test-gate-$tree"\n`,
    );
    run('git', ['add', 'scripts/check.sh'], f.repo);
    run('git', ['commit', '-m', 'Fixture gate'], f.repo);
    const remote = resolve(f.root, 'remote.git');
    run('git', ['init', '--bare', remote], f.root);
    run('git', ['remote', 'add', 'origin', remote], f.repo);
    run('git', ['config', 'core.hooksPath', '.githooks'], f.repo);
    const env = {
      ...withoutGitRepositoryEnv(process.env),
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
    assert.throws(() => run('git', ['show-ref', '--verify', 'refs/hotsheet/main-integration-window'], f.repo));
    rmSync(fail);
    assert.throws(() => run('git', ['push', 'origin', 'main'], f.repo, env));
    run('bash', ['scripts/push.sh'], f.repo, env);
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
    const env = {
      ...withoutGitRepositoryEnv(process.env),
      PATH: `${bin}:${process.env.PATH}`,
      HOTSHEET_HOOK_TEST_LOG: log,
    };
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

test('HS2-9EGWC4 pre-commit checks only whitespace for a registry-only commit', () => {
  const f = fixture();
  try {
    const bin = resolve(f.root, 'bin');
    const log = resolve(f.root, 'format.log');
    executable(resolve(bin, 'zsh'), '#!/bin/sh\nprintf "format\\n" >> "$HOTSHEET_HOOK_TEST_LOG"\n');
    run('git', ['config', 'core.hooksPath', '.githooks'], f.repo);
    const env = {
      ...withoutGitRepositoryEnv(process.env),
      PATH: `${bin}:${process.env.PATH}`,
      HOTSHEET_HOOK_TEST_LOG: log,
    };
    // Unrelated staged work stays out of Hot Sheet's path-limited commit, which skips the format check.
    put(resolve(f.repo, 'code.ts'), 'staged\n');
    run('git', ['add', 'code.ts'], f.repo);
    put(resolve(f.repo, '.hotsheet2/groups/7K3M9P2QXA.json'), '{\n  "name": "Sprint"\n}\n');
    run('git', ['add', '.hotsheet2/groups/7K3M9P2QXA.json'], f.repo);
    run(
      'git',
      ['commit', '-q', '--only', '-m', 'Create group Sprint', '--', '.hotsheet2/groups/7K3M9P2QXA.json'],
      f.repo,
      env,
    );
    assert.equal(existsSync(log), false);
    assert.equal(run('git', ['diff', '--cached', '--name-only'], f.repo).trim(), 'code.ts');
    // Whitespace still fails a registry-only commit.
    put(resolve(f.repo, '.hotsheet2/groups/7K3M9P2QXB.json'), '{"name": "Bad"}  \n');
    run('git', ['add', '.hotsheet2/groups/7K3M9P2QXB.json'], f.repo);
    assert.throws(() =>
      run('git', ['commit', '-q', '--only', '-m', 'Bad', '--', '.hotsheet2/groups/7K3M9P2QXB.json'], f.repo, env),
    );
    // Any other path keeps the full gate, even beside registry files.
    put(resolve(f.repo, '.hotsheet2/groups/7K3M9P2QXB.json'), '{"name": "Good"}\n');
    run('git', ['add', '.hotsheet2/groups/7K3M9P2QXB.json'], f.repo);
    run('git', ['commit', '-q', '-m', 'Mixed'], f.repo, env);
    assert.equal(readFileSync(log, 'utf8'), 'format\n');
  } finally {
    f.dispose();
  }
});

test('HS2-9EGWC4 pre-push lets only a registry-only fast-forward skip the gate and window', () => {
  const f = fixture();
  try {
    // Without a reservation the main-window verification (run through zsh) refuses.
    const bin = resolve(f.root, 'bin');
    executable(resolve(bin, 'zsh'), '#!/bin/sh\nexit 1\n');
    const log = resolve(f.root, 'gate.log');
    // Fixture commits bypass the hooks by pointing at an empty hooks directory.
    const noHooks = resolve(f.root, 'no-hooks');
    mkdirSync(noHooks);
    const commit = (args) => run('git', ['-c', `core.hooksPath=${noHooks}`, 'commit', '-q', ...args], f.repo);
    executable(
      resolve(f.repo, 'scripts/check.sh'),
      '#!/bin/sh\nprintf "check\\n" >> "$HOTSHEET_HOOK_TEST_LOG"\nexit 1\n',
    );
    run('git', ['add', 'scripts/check.sh'], f.repo);
    run('git', ['commit', '-q', '-m', 'Failing fixture gate'], f.repo);
    const remote = resolve(f.root, 'remote.git');
    run('git', ['init', '-q', '--bare', remote], f.root);
    run('git', ['remote', 'add', 'origin', remote], f.repo);
    run('git', ['push', '-q', 'origin', 'main', 'main:refs/heads/feature'], f.repo);
    run('git', ['config', 'core.hooksPath', '.githooks'], f.repo);
    const env = {
      ...withoutGitRepositoryEnv(process.env),
      PATH: `${bin}:${process.env.PATH}`,
      HOTSHEET_HOOK_TEST_LOG: log,
    };
    const remoteMain = () => run('git', ['rev-parse', 'refs/heads/main'], remote).trim();
    const entry = (id, name) => {
      put(resolve(f.repo, `.hotsheet2/groups/${id}.json`), `{\n  "name": "${name}"\n}\n`);
      run('git', ['add', `.hotsheet2/groups/${id}.json`], f.repo);
      commit(['-m', `Create group ${name}`]);
    };
    // Two registry commits go to main without a reservation and without the gate.
    entry('7K3M9P2QXA', 'Sprint');
    entry('7K3M9P2QXB', 'Backlog');
    run('git', ['push', '-q', 'origin', 'main'], f.repo, env);
    assert.equal(remoteMain(), run('git', ['rev-parse', 'HEAD'], f.repo).trim());
    // The same holds for another branch.
    run('git', ['checkout', '-q', '-b', 'feature', 'origin/feature'], f.repo);
    entry('7K3M9P2QXC', 'Feature');
    run('git', ['push', '-q', 'origin', 'feature'], f.repo, env);
    assert.equal(existsSync(log), false);
    run('git', ['checkout', '-q', 'main'], f.repo);
    // A code change in the range keeps the main reservation requirement.
    const published = remoteMain();
    put(resolve(f.repo, 'code.ts'), 'code\n');
    run('git', ['add', 'code.ts'], f.repo);
    commit(['-m', 'Code']);
    entry('7K3M9P2QXD', 'After code');
    assert.throws(() => run('git', ['push', '-q', 'origin', 'main'], f.repo, env));
    assert.equal(remoteMain(), published);
    // A new branch has no base to compare, so it takes the gate.
    run('git', ['checkout', '-q', '-b', 'fresh', published], f.repo);
    entry('7K3M9P2QXE', 'Fresh');
    assert.throws(() => run('git', ['push', '-q', 'origin', 'fresh'], f.repo, env));
    assert.equal(readFileSync(log, 'utf8'), 'check\n');
    // A forced rewrite is never a registry-only fast-forward.
    run('git', ['checkout', '-q', 'main'], f.repo);
    run('git', ['reset', '-q', '--hard', published], f.repo);
    run('git', ['reset', '-q', '--hard', 'HEAD~1'], f.repo);
    entry('7K3M9P2QXF', 'Rewritten');
    assert.throws(() => run('git', ['push', '-q', '--force', 'origin', 'main'], f.repo, env));
    assert.equal(remoteMain(), published);
  } finally {
    f.dispose();
  }
});
