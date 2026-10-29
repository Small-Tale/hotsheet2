import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync, spawn } from 'node:child_process';
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { verifyMainPush } from './main-integration-window.mjs';

const source = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const run = (program, args, cwd, env = process.env, input) =>
  execFileSync(program, args, { cwd, env, input, encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] }).trim();
const put = (path, contents, executable = false) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, contents);
  if (executable) chmodSync(path, 0o755);
};
const gateScript = `#!/bin/sh
tree=$(git rev-parse 'HEAD^{tree}')
verify=0
if [ "\${1:-}" = '--verify-receipt' ]; then verify=1; shift; fi
selection=$(printf '%s\\n' "$@" | shasum -a 256 | cut -c 1-12)
marker=$(git rev-parse --git-path "test-gate-$tree-$selection")
if [ "$verify" -eq 1 ]; then test -f "$marker"; exit; fi
if [ -f "$marker" ]; then exit 0; fi
printf 'gate\\n' >> "$HS2_TEST_GATE_LOG"
if [ -n "\${HS2_TEST_HOLD:-}" ]; then
  touch "$HS2_TEST_STARTED"
  while [ -f "$HS2_TEST_HOLD" ]; do sleep 0.1; done
fi
if [ -f "$HS2_TEST_FAIL" ]; then exit 1; fi
touch "$marker"
`;

function fixture() {
  const root = mkdtempSync(resolve(tmpdir(), 'hs2-bnd42n-'));
  const repo = resolve(root, 'repo');
  const remote = resolve(root, 'remote.git');
  const bin = resolve(root, 'bin');
  mkdirSync(repo);
  run('git', ['init', '-b', 'main'], repo);
  run('git', ['config', 'user.name', 'Hot Sheet Test'], repo);
  run('git', ['config', 'user.email', 'test@invalid.example'], repo);
  for (const path of [
    'scripts/push.sh',
    'scripts/main-integration-window.mjs',
    'scripts/main-integration-options.mjs',
    '.githooks/pre-commit',
    '.githooks/pre-push',
  ]) {
    const target = resolve(repo, path);
    mkdirSync(dirname(target), { recursive: true });
    copyFileSync(resolve(source, path), target);
    if (path.endsWith('.sh') || path.includes('.githooks/')) chmodSync(target, 0o755);
  }
  for (const path of ['clients/web', 'migrator', 'spikes/kerf-webawesome'])
    mkdirSync(resolve(repo, path), { recursive: true });
  put(resolve(repo, 'scripts/check.sh'), gateScript, true);
  put(resolve(repo, 'seed'), 'seed\n');
  run(
    'git',
    [
      'add',
      'scripts/push.sh',
      'scripts/main-integration-window.mjs',
      'scripts/main-integration-options.mjs',
      '.githooks/pre-commit',
      '.githooks/pre-push',
      'scripts/check.sh',
      'seed',
    ],
    repo,
  );
  run('git', ['commit', '-m', 'Synthetic fixture'], repo);
  run('git', ['init', '--bare', '-b', 'main', remote], root);
  run('git', ['remote', 'add', 'origin', remote], repo);
  put(
    resolve(bin, 'zsh'),
    '#!/bin/sh\nif [ "$2" = "npm run format:check" ]; then exit 0; fi\nexec /bin/zsh "$@"\n',
    true,
  );
  const env = {
    ...process.env,
    PATH: `${bin}:${process.env.PATH}`,
    HS2_TEST_GATE_LOG: resolve(root, 'gate.log'),
    HS2_TEST_FAIL: resolve(root, 'fail'),
    HS2_TEST_HOLD: '',
    HS2_TEST_STARTED: resolve(root, 'started'),
  };
  run('git', ['config', 'core.hooksPath', '.githooks'], repo);
  return { root, repo, remote, env, dispose: () => rmSync(root, { recursive: true, force: true }) };
}

function commit(repo, path, env) {
  put(resolve(repo, path), `${path}\n`);
  run('git', ['add', path], repo, env);
  return run('git', ['commit', '-m', `Synthetic ${path}`], repo, env);
}

function runAsync(program, args, cwd, env) {
  const child = spawn(program, args, { cwd, env, detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
  let output = '';
  child.stdout.on('data', (part) => {
    output += part;
  });
  child.stderr.on('data', (part) => {
    output += part;
  });
  return {
    child,
    completed: new Promise((done) => child.once('close', (code) => done({ code, output }))),
  };
}

async function waitForFile(path) {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (existsSync(path)) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  throw new Error('Timed out waiting for the first gate to start');
}

test('HS2-BND42N expired owner is replaced and its ref is released', () => {
  const f = fixture();
  try {
    run('git', ['config', 'core.hooksPath', resolve(f.repo, '.githooks')], f.repo);
    run('bash', ['scripts/push.sh'], f.repo, f.env);
    commit(f.repo, 'next', f.env);
    const head = run('git', ['rev-parse', 'HEAD'], f.repo);
    const tree = run('git', ['rev-parse', 'HEAD^{tree}'], f.repo);
    const old = JSON.stringify({
      version: 1,
      token: 'expired',
      pid: 1,
      expiresAt: 0,
      phase: 'gating',
      remote: 'origin',
      target: 'refs/heads/main',
      head,
      tree,
    });
    const blob = run('git', ['hash-object', '-w', '--stdin'], f.repo, f.env, old);
    run('git', ['update-ref', 'refs/hotsheet/main-integration-window', blob], f.repo);
    assert.throws(
      () => verifyMainPush(f.repo, 'expired', 'origin', 'refs/heads/main', head, '0'.repeat(40)),
      /lost or expired/,
    );
    run('bash', ['scripts/push.sh'], f.repo, f.env);
    assert.equal(run('git', ['rev-parse', 'refs/heads/main'], f.remote), head);
    assert.throws(() => run('git', ['show-ref', '--verify', 'refs/hotsheet/main-integration-window'], f.repo));
  } finally {
    f.dispose();
  }
});

test('HS2-BND42N remote writer rejects local gate before it starts', () => {
  const f = fixture();
  try {
    run('bash', ['scripts/push.sh'], f.repo, f.env);
    commit(f.repo, 'local', f.env);
    const writer = resolve(f.root, 'writer');
    run('git', ['clone', f.remote, writer], f.root);
    run('git', ['config', 'user.name', 'Remote Test'], writer);
    run('git', ['config', 'user.email', 'remote@invalid.example'], writer);
    commit(writer, 'remote', f.env);
    run('git', ['push', 'origin', 'main'], writer);
    const tip = run('git', ['rev-parse', 'refs/heads/main'], f.remote);
    assert.throws(() => run('bash', ['scripts/push.sh'], f.repo, f.env));
    assert.equal(run('git', ['rev-parse', 'refs/heads/main'], f.remote), tip);
    assert.equal(readFileSync(f.env.HS2_TEST_GATE_LOG, 'utf8'), 'gate\n');
  } finally {
    f.dispose();
  }
});

test('HS2-BND42N a live pool fences standalone push without mutating its state', () => {
  const f = fixture();
  try {
    const canonical = realpathSync(f.repo);
    const projectId = `repo-${createHash('sha256').update(canonical).digest('hex').slice(0, 12)}`;
    const sourceId = createHash('sha256').update(canonical).digest('hex');
    const state = JSON.stringify({
      version: 6,
      project_id: projectId,
      source_id: sourceId,
      stopped: null,
      integrator: { id: 'i', generation: 'g', expires_at: Math.floor(Date.now() / 1000) + 600 },
      assignments: {},
    });
    const pool = resolve(f.repo, '.git/hotsheet-coordination/state.json');
    put(pool, state);
    assert.throws(() => run('bash', ['scripts/push.sh'], f.repo, f.env));
    assert.equal(existsSync(f.env.HS2_TEST_GATE_LOG), false);
    run('bash', ['scripts/push.sh', '--integrator-id', 'i', '--integrator-session', 'g'], f.repo, f.env);
    assert.equal(readFileSync(pool, 'utf8'), state);
  } finally {
    f.dispose();
  }
});

test('HS2-BND42N ticket phase advances at the gate and confirmed-push boundaries', () => {
  const f = fixture();
  try {
    const cli = resolve(f.root, 'bin/hotsheet-cli');
    const phases = resolve(f.root, 'phases.log');
    put(
      cli,
      '#!/bin/sh\nwhile [ "$#" -gt 0 ]; do\n  if [ "$1" = "--started-phase" ]; then printf "%s\\n" "$2" >> "$HS2_TEST_PHASES"; exit 0; fi\n  shift\ndone\nexit 1\n',
      true,
    );
    const env = { ...f.env, HOTSHEET_MAIN_WINDOW_CLI: cli, HS2_TEST_PHASES: phases };
    run('bash', ['scripts/push.sh', '--gate-arg', '--ticket', '--gate-arg', 'HS2-BND42N'], f.repo, env);
    assert.equal(readFileSync(phases, 'utf8'), 'integrating\nfinal_testing\n');
  } finally {
    f.dispose();
  }
});

test('HS2-BND42N full selection is verified again inside the push hook', () => {
  const f = fixture();
  try {
    run('bash', ['scripts/push.sh', '--gate-arg', '--full'], f.repo, f.env);
    assert.equal(readFileSync(f.env.HS2_TEST_GATE_LOG, 'utf8'), 'gate\n');
  } finally {
    f.dispose();
  }
});

test('HS2-BND42N a remote writer during the gate fails the second preflight', { timeout: 60_000 }, async () => {
  const f = fixture();
  let running;
  try {
    run('bash', ['scripts/push.sh'], f.repo, f.env);
    commit(f.repo, 'local', f.env);
    const writer = resolve(f.root, 'writer');
    run('git', ['clone', f.remote, writer], f.root);
    run('git', ['config', 'user.name', 'Remote Test'], writer);
    run('git', ['config', 'user.email', 'remote@invalid.example'], writer);
    commit(writer, 'remote', f.env);
    const hold = resolve(f.root, 'hold');
    put(hold, 'hold');
    running = runAsync('bash', ['scripts/push.sh'], f.repo, { ...f.env, HS2_TEST_HOLD: hold });
    await waitForFile(f.env.HS2_TEST_STARTED);
    run('git', ['push', 'origin', 'main'], writer);
    const tip = run('git', ['rev-parse', 'refs/heads/main'], f.remote);
    rmSync(hold);
    const result = await running.completed;
    assert.notEqual(result.code, 0, result.output);
    assert.match(result.output, /not an ancestor/);
    assert.equal(run('git', ['rev-parse', 'refs/heads/main'], f.remote), tip);
  } finally {
    if (running?.child.exitCode === null)
      try {
        process.kill(-running.child.pid, 'SIGTERM');
      } catch {
        /* exited */
      }
    f.dispose();
  }
});

test('HS2-BND42N a changed local HEAD during the gate cannot be pushed', { timeout: 60_000 }, async () => {
  const f = fixture();
  let running;
  try {
    run('bash', ['scripts/push.sh'], f.repo, f.env);
    const remoteBase = run('git', ['rev-parse', 'refs/heads/main'], f.remote);
    commit(f.repo, 'first', f.env);
    const hold = resolve(f.root, 'hold');
    put(hold, 'hold');
    running = runAsync('bash', ['scripts/push.sh'], f.repo, { ...f.env, HS2_TEST_HOLD: hold });
    await waitForFile(f.env.HS2_TEST_STARTED);
    commit(f.repo, 'changed', f.env);
    rmSync(hold);
    const result = await running.completed;
    assert.notEqual(result.code, 0, result.output);
    assert.match(result.output, /checked-out HEAD|Checked-out main changed|check\.sh failed/);
    assert.equal(run('git', ['rev-parse', 'refs/heads/main'], f.remote), remoteBase);
  } finally {
    if (running?.child.exitCode === null)
      try {
        process.kill(-running.child.pid, 'SIGTERM');
      } catch {
        /* exited */
      }
    f.dispose();
  }
});

test('HS2-BND42N two worktrees serialize the gate and reject the superseded writer', { timeout: 60_000 }, async () => {
  const f = fixture();
  let first;
  let second;
  try {
    run('bash', ['scripts/push.sh'], f.repo, f.env);
    const other = resolve(f.root, 'other');
    run('git', ['worktree', 'add', '-b', 'contender', other], f.repo);
    for (const path of ['clients/web', 'migrator', 'spikes/kerf-webawesome'])
      mkdirSync(resolve(other, path), { recursive: true });
    commit(f.repo, 'first', f.env);
    commit(other, 'second', f.env);
    const hold = resolve(f.root, 'hold');
    put(hold, 'hold');
    first = runAsync('bash', ['scripts/push.sh'], f.repo, { ...f.env, HS2_TEST_HOLD: hold });
    await waitForFile(f.env.HS2_TEST_STARTED);
    second = runAsync('bash', ['scripts/push.sh', 'origin', 'HEAD:main'], other, f.env);
    await new Promise((done) => setTimeout(done, 350));
    assert.equal(readFileSync(f.env.HS2_TEST_GATE_LOG, 'utf8'), 'gate\ngate\n');
    rmSync(hold);
    const [winner, loser] = await Promise.all([first.completed, second.completed]);
    assert.equal(winner.code, 0, winner.output);
    assert.notEqual(loser.code, 0, loser.output);
    assert.match(loser.output, /not an ancestor/);
    assert.equal(readFileSync(f.env.HS2_TEST_GATE_LOG, 'utf8'), 'gate\ngate\n');
  } finally {
    for (const running of [first, second])
      if (running?.child.exitCode === null)
        try {
          process.kill(-running.child.pid, 'SIGTERM');
        } catch {
          /* exited */
        }
    f.dispose();
  }
});
