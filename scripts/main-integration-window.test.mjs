import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync, spawn, spawnSync } from 'node:child_process';
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
import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';
import { acceptedHooksPath, gateDiagnostics, readWindow, selectedSourceStore, verifyMainPush } from './main-integration-window.mjs';

const source = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const run = (program, args, cwd, env = withoutGitRepositoryEnv(process.env), input) =>
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
  printf '%s\\n' "$$" > "$HS2_TEST_STARTED"
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
    'clients/web/scripts/repository-env.mjs',
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
      'clients/web/scripts/repository-env.mjs',
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
    // Linux runners have no /bin/zsh; the wrapped commands are POSIX sh (HS2-KKDZF4).
    '#!/bin/sh\nif [ "$2" = "npm run format:check" ]; then exit 0; fi\nif [ -x /bin/zsh ]; then exec /bin/zsh "$@"; fi\nexec /bin/sh "$@"\n',
    true,
  );
  const env = {
    ...withoutGitRepositoryEnv(process.env),
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

/**
 * Wait for the held gate's started marker, or fail with the push's output if it exits first.
 * The test's own timeout bounds the wait; a fixed attempt cap timed out under full-gate load (HS2-Z5NYGH).
 */
async function waitForFile(path, running) {
  let closed;
  void running.completed.then((result) => {
    closed = result;
  });
  while (!existsSync(path)) {
    if (closed) throw new Error(`push exited (${closed.code}) before its gate started:\n${closed.output}`);
    await new Promise((done) => setTimeout(done, 100));
  }
}

test('HS2-W110HQ release or replacement after lookup preserves one immutable reservation snapshot', () => {
  const f = fixture();
  const ref = 'refs/hotsheet/main-integration-window';
  try {
    const value = {
      version: 1,
      token: 'synthetic-original',
      pid: process.pid,
      expiresAt: Date.now() + 60_000,
      phase: 'gating',
      remote: 'origin',
      target: 'refs/heads/main',
      head: run('git', ['rev-parse', 'HEAD'], f.repo),
      tree: run('git', ['rev-parse', 'HEAD^{tree}'], f.repo),
    };
    const oid = run('git', ['hash-object', '-w', '--stdin'], f.repo, f.env, JSON.stringify(value));
    const replacement = run(
      'git',
      ['hash-object', '-w', '--stdin'],
      f.repo,
      f.env,
      JSON.stringify({ ...value, token: 'synthetic-replacement' }),
    );
    for (const next of [null, replacement]) {
      run('git', ['update-ref', ref, oid], f.repo);
      let lookups = 0;
      const snapshot = readWindow(f.repo, (repo, args) => {
        lookups++;
        const result = spawnSync('git', args, {
          cwd: repo,
          env: withoutGitRepositoryEnv(process.env),
          encoding: 'utf8',
        });
        assert.equal(result.status, 0, result.stderr);
        if (next) run('git', ['update-ref', ref, next, oid], repo);
        else run('git', ['update-ref', '-d', ref, oid], repo);
        return result;
      });
      assert.deepEqual(snapshot, { oid, value });
      assert.equal(lookups, 1);
      assert.equal(readWindow(f.repo)?.value.token ?? null, next ? 'synthetic-replacement' : null);
    }
  } finally {
    f.dispose();
  }
});

test('HS2-W110HQ absent reservations differ from Git failures and malformed snapshots', () => {
  const f = fixture();
  try {
    assert.equal(readWindow(f.repo), null);
    assert.throws(() => readWindow(f.repo, () => ({ status: 128, stderr: 'synthetic failure' })), /Cannot read/);
    assert.throws(
      () => readWindow(f.repo, () => ({ status: null, error: new Error('synthetic spawn failure') })),
      /Cannot read/,
    );
    assert.throws(() => readWindow(f.repo, () => ({ status: 0, stdout: 'not-an-object-id' })), /invalid object id/);
    const ref = 'refs/hotsheet/main-integration-window';
    for (const contents of ['not json', JSON.stringify({ version: 99 })]) {
      const oid = run('git', ['hash-object', '-w', '--stdin'], f.repo, f.env, contents);
      run('git', ['update-ref', ref, oid], f.repo);
      assert.throws(() => readWindow(f.repo));
    }
  } finally {
    f.dispose();
  }
});

test('HS2-822EVF retries one transient ref read and keeps a valid reservation', () => {
  const f = fixture();
  const ref = 'refs/hotsheet/main-integration-window';
  try {
    const value = {
      version: 1,
      token: 'synthetic-transient',
      pid: process.pid,
      expiresAt: Date.now() + 60_000,
      phase: 'gating',
      remote: 'origin',
      target: 'refs/heads/main',
      head: run('git', ['rev-parse', 'HEAD'], f.repo),
      tree: run('git', ['rev-parse', 'HEAD^{tree}'], f.repo),
    };
    const oid = run('git', ['hash-object', '-w', '--stdin'], f.repo, f.env, JSON.stringify(value));
    run('git', ['update-ref', ref, oid], f.repo, f.env);
    let reads = 0;
    const result = readWindow(f.repo, (repo, args) => {
      reads++;
      if (reads === 1) return { status: 128, stderr: 'synthetic transient read failure' };
      return spawnSync('git', args, { cwd: repo, env: withoutGitRepositoryEnv(process.env), encoding: 'utf8' });
    });
    assert.equal(reads, 2);
    assert.deepEqual(result, { oid, value });
    assert.throws(
      () => readWindow(f.repo, () => ({ status: 128, stderr: 'persistent synthetic failure' })),
      /Cannot read main integration ref: persistent synthetic failure/,
    );
    assert.deepEqual(
      readWindow(f.repo, (repo, args) => {
        const result = spawnSync('git', args, { cwd: repo, env: withoutGitRepositoryEnv(process.env), encoding: 'utf8' });
        return { ...result, stderr: 'benign Git warning' };
      }),
      { oid, value },
    );
  } finally {
    f.dispose();
  }
});

test('HS2-822EVF selects the linked ticket store and preserves an explicit source override', () => {
  const f = fixture();
  try {
    const store = resolve(f.root, 'tickets');
    assert.equal(selectedSourceStore(f.repo), f.repo);
    put(resolve(f.repo, '.hotsheet2/store'), `${store}\n`);
    assert.equal(selectedSourceStore(f.repo), store);
    assert.equal(selectedSourceStore(f.repo, f.repo), f.repo);
    put(resolve(f.repo, '.hotsheet2/store'), 'relative/store\n');
    assert.throws(() => selectedSourceStore(f.repo), /absolute store path/);
  } finally {
    f.dispose();
  }
});

for (const shape of [
  'absent',
  'valid_blob',
  'empty',
  'malformed_text',
  'short_oid',
  'nonhex_40',
  'zero_40',
  'missing_object_40',
  'dangling_symref',
  'cyclic_symref',
  'invalid_symref_target',
  'valid_symref',
  'directory_at_ref',
  'invalid_packed_oid',
])
  test(`HS2-BYDMV4 real Git physical ref ${shape} cannot disguise corruption as absence`, () => {
    const f = fixture(),
      ref = 'refs/hotsheet/main-integration-window';
    try {
      const value = {
        version: 1,
        token: 'synthetic-physical-ref',
        pid: process.pid,
        expiresAt: Date.now() + 60_000,
        phase: 'gating',
        remote: 'origin',
        target: 'refs/heads/main',
        head: run('git', ['rev-parse', 'HEAD'], f.repo),
        tree: run('git', ['rev-parse', 'HEAD^{tree}'], f.repo),
      };
      const oid = run('git', ['hash-object', '-w', '--stdin'], f.repo, f.env, JSON.stringify(value)),
        path = resolve(f.repo, run('git', ['rev-parse', '--git-path', ref], f.repo));
      const contents = {
        empty: '',
        malformed_text: 'not an object\n',
        short_oid: `${oid.slice(0, 12)}\n`,
        nonhex_40: `${'g'.repeat(40)}\n`,
        zero_40: `${'0'.repeat(40)}\n`,
        missing_object_40: `${'1'.repeat(40)}\n`,
        dangling_symref: 'ref: refs/hotsheet/missing\n',
        cyclic_symref: `ref: ${ref}\n`,
        invalid_symref_target: 'ref: refs/hotsheet/invalid..target\n',
      };
      if (Object.hasOwn(contents, shape)) put(path, contents[shape]);
      else if (shape === 'valid_blob') run('git', ['update-ref', ref, oid], f.repo);
      else if (shape === 'valid_symref') {
        run('git', ['update-ref', 'refs/hotsheet/valid-target', oid], f.repo);
        run('git', ['symbolic-ref', ref, 'refs/hotsheet/valid-target'], f.repo);
      } else if (shape === 'directory_at_ref') mkdirSync(path, { recursive: true });
      else if (shape === 'invalid_packed_oid')
        put(
          resolve(f.repo, run('git', ['rev-parse', '--git-path', 'packed-refs'], f.repo)),
          `${'g'.repeat(40)} ${ref}\n`,
        );
      if (shape === 'absent') assert.equal(readWindow(f.repo), null, shape);
      else if (shape === 'valid_blob' || shape === 'valid_symref')
        assert.deepEqual(readWindow(f.repo), { oid, value }, shape);
      else assert.throws(() => readWindow(f.repo), undefined, shape);
    } finally {
      f.dispose();
    }
  });

test('HS2-BYDMV4 unresolved lookup diagnostics and subprocess faults cannot become absence', () => {
  for (const result of [
    { status: 1, stderr: 'synthetic diagnostic in any language' },
    { status: 1, stderr: '\n' },
    { status: 1, stdout: '\n' },
    { status: 1, error: new Error('synthetic timeout') },
    { status: 1, signal: 'SIGKILL' },
    { status: null, error: new Error('synthetic spawn failure') },
    { status: 128 },
    { status: 0, stdout: 'invalid-object-id' },
  ]) {
    let queries = 0;
    assert.throws(
      () =>
        readWindow('unused', () => {
          queries++;
          return result;
        }),
      /Cannot read/,
    );
    assert.equal(queries, 1, 'a hard lookup failure never invokes the existence fallback');
  }
});

test('HS2-BYDMV4 fallback uses typed absence and rejects unsupported or failed capability', () => {
  const ref = 'refs/hotsheet/main-integration-window';
  const f = fixture();
  try {
    for (const stderr of ['', 'ordinary typed-missing diagnostic without language interpretation']) {
      const calls = [];
      assert.equal(
        readWindow(f.repo, (_repo, args) => {
          calls.push(args);
          return calls.length === 1 ? { status: 1, stdout: '', stderr: '' } : { status: 2, stderr };
        }),
        null,
      );
      assert.deepEqual(calls, [
        ['rev-parse', '--verify', '--quiet', ref],
        ['show-ref', '--exists', ref],
      ]);
    }
    for (const result of [
      { status: 1, stderr: 'synthetic lookup failure' },
      { status: 129, stderr: 'synthetic unsupported option' },
      { status: null, error: new Error('synthetic spawn failure') },
      { status: 2, error: new Error('synthetic timeout') },
      { status: 2, signal: 'SIGTERM' },
      { status: 2, stdout: 'unexpected output' },
      { status: 0, stderr: 'synthetic diagnostic' },
      { status: 0, stdout: 'unexpected output' },
    ]) {
      let calls = 0;
      assert.throws(() => readWindow('unused', () => (++calls === 1 ? { status: 1 } : result)), /Cannot determine/);
      assert.equal(calls, 2);
    }
  } finally {
    f.dispose();
  }
});

test('HS2-BYDMV4 a new owner between unresolved lookup and existence requires a fresh snapshot', () => {
  for (const release of [false, true]) {
    const f = fixture(),
      ref = 'refs/hotsheet/main-integration-window';
    try {
      const value = {
        version: 1,
        token: 'synthetic-raced-owner',
        pid: process.pid,
        expiresAt: Date.now() + 60_000,
        phase: 'gating',
        remote: 'origin',
        target: 'refs/heads/main',
        head: run('git', ['rev-parse', 'HEAD'], f.repo),
        tree: run('git', ['rev-parse', 'HEAD^{tree}'], f.repo),
      };
      const oid = run('git', ['hash-object', '-w', '--stdin'], f.repo, f.env, JSON.stringify(value));
      let queries = 0;
      const read = () =>
        readWindow(f.repo, (repo, args) => {
          const result = spawnSync('git', args, {
            cwd: repo,
            env: withoutGitRepositoryEnv(process.env),
            encoding: 'utf8',
          });
          if (++queries === 1) {
            assert.equal(result.status, 1);
            assert.equal(result.stderr, '');
            run('git', ['update-ref', ref, oid], repo);
            if (release) run('git', ['update-ref', '-d', ref, oid], repo);
          }
          return result;
        });
      if (release) assert.equal(read(), null);
      else {
        assert.throws(read, (error) => error.constructor.name === 'UnresolvedWindowError');
        assert.deepEqual(readWindow(f.repo), { oid, value });
      }
      assert.equal(queries, 2);
    } finally {
      f.dispose();
    }
  }
});

test('HS2-BYDMV4 malformed blobs and nonblob objects remain hard failures', () => {
  const f = fixture(),
    ref = 'refs/hotsheet/main-integration-window';
  try {
    for (const contents of ['not JSON', JSON.stringify({ version: 99 })]) {
      const oid = run('git', ['hash-object', '-w', '--stdin'], f.repo, f.env, contents);
      run('git', ['update-ref', ref, oid], f.repo);
      assert.throws(() => readWindow(f.repo));
    }
    run('git', ['update-ref', ref, 'HEAD'], f.repo);
    assert.throws(() => readWindow(f.repo));
  } finally {
    f.dispose();
  }
});

/** Test-owned quiet lookup faults and optional real competing CAS; other Git operations stay real. */
function unresolvedRefEnvironment(f, persistent = false) {
  const realGit = run('sh', ['-c', 'command -v git'], f.repo, f.env),
    lookup = resolve(f.root, 'unresolved-lookup'),
    present = resolve(f.root, 'presence-observed');
  put(
    resolve(f.root, 'bin/git'),
    `#!/bin/sh
if [ "$#" -eq 4 ] && [ "$1" = rev-parse ] && [ "$2" = --verify ] && [ "$3" = --quiet ] && [ "$4" = refs/hotsheet/main-integration-window ]; then
  if { [ -z "$HS2_TEST_REF_CAS_FAILURE" ] || [ -e "$HS2_TEST_REF_CAS_FAILURE" ]; } && { [ "$HS2_TEST_REF_PERSISTENT" = 1 ] || [ ! -e "$HS2_TEST_REF_LOOKUP" ]; }; then
    : > "$HS2_TEST_REF_LOOKUP"
    exit 1
  fi
  : > "$HS2_TEST_REF_RESOLVED"
fi
if [ "$#" -eq 3 ] && [ "$1" = show-ref ] && [ "$2" = --exists ] && [ "$3" = refs/hotsheet/main-integration-window ]; then
  : > "$HS2_TEST_REF_PRESENT"
fi
if [ -n "$HS2_TEST_REF_CAS_FAILURE" ] && [ ! -e "$HS2_TEST_REF_CAS_FAILURE" ] && [ "$#" -eq 6 ] && [ "$1" = -c ] && [ "$2" = core.filesRefLockTimeout=1000 ] && [ "$3" = update-ref ] && [ "$4" = refs/hotsheet/main-integration-window ]; then
  "$HS2_TEST_REAL_GIT" update-ref "$4" "$HS2_TEST_REF_COMPETITOR" "$6" || exit 1
  "$HS2_TEST_REAL_GIT" "$@"
  result_code=$?
  printf '%s\\n' "$result_code" > "$HS2_TEST_REF_CAS_FAILURE"
  exit "$result_code"
fi
exec "$HS2_TEST_REAL_GIT" "$@"
`,
    true,
  );
  return {
    ...f.env,
    HS2_TEST_REAL_GIT: realGit,
    HS2_TEST_REF_LOOKUP: lookup,
    HS2_TEST_REF_PRESENT: present,
    HS2_TEST_REF_PERSISTENT: persistent ? '1' : '0',
    HS2_TEST_REF_RESOLVED: resolve(f.root, 'resolved-lookup'),
    HS2_TEST_REF_CAS_FAILURE: '',
    HS2_TEST_REF_COMPETITOR: '',
  };
}

function expiredRef(f) {
  const ref = 'refs/hotsheet/main-integration-window',
    head = run('git', ['rev-parse', 'HEAD'], f.repo),
    value = {
      version: 1,
      token: 'synthetic-expired-owner',
      pid: process.pid,
      expiresAt: 0,
      phase: 'gating',
      remote: 'origin',
      target: 'refs/heads/main',
      head,
      tree: run('git', ['rev-parse', 'HEAD^{tree}'], f.repo),
    },
    oid = run('git', ['hash-object', '-w', '--stdin'], f.repo, f.env, JSON.stringify(value));
  run('git', ['update-ref', ref, oid], f.repo);
  return { oid, head, ref };
}

test('HS2-BYDMV4 actual push retries unresolved presence and replaces only its fresh captured owner', () => {
  const f = fixture();
  try {
    const { head, ref } = expiredRef(f),
      env = unresolvedRefEnvironment(f),
      module = new URL('./main-integration-window.mjs', import.meta.url).href;
    // Exercise shipped pushMain and real push/hooks after launch, keeping this query fault in its env.
    run(
      process.execPath,
      [
        '--input-type=module',
        '-e',
        `
import { pushMain } from ${JSON.stringify(module)};
await pushMain(${JSON.stringify(f.repo)}, []);
`,
      ],
      f.repo,
      env,
    );
    assert.equal(readFileSync(env.HS2_TEST_REF_LOOKUP, 'utf8'), '');
    assert.equal(readFileSync(env.HS2_TEST_REF_PRESENT, 'utf8'), '');
    assert.equal(run('git', ['rev-parse', 'refs/heads/main'], f.remote), head);
    assert.equal(readWindow(f.repo), null);
    assert.throws(() => run('git', ['show-ref', '--verify', ref], f.repo));
  } finally {
    f.dispose();
  }
});

test('HS2-BYDMV4 existing acquisition budget expires with an actionable unresolved-ref error', () => {
  const f = fixture();
  try {
    const { oid, ref } = expiredRef(f),
      env = unresolvedRefEnvironment(f, true),
      module = new URL('./main-integration-window.mjs', import.meta.url).href,
      program = `
import { existsSync } from 'node:fs';
import { pushMain } from ${JSON.stringify(module)};
const originalNow = Date.now;
const started = originalNow();
// Advance only after the real existence query was reached, never before admission began.
Date.now = () => existsSync(process.env.HS2_TEST_REF_PRESENT) ? started + 10 * 60_000 + 1 : started;
try { await pushMain(${JSON.stringify(f.repo)}, []); }
catch (error) { console.error(error.message); process.exitCode = 1; }
finally { Date.now = originalNow; }
`;
    const result = spawnSync(process.execPath, ['--input-type=module', '-e', program], {
      cwd: f.repo,
      env,
      encoding: 'utf8',
      timeout: 15_000,
    });
    assert.ifError(result.error);
    assert.equal(result.status, 1);
    assert.equal(result.signal, null);
    assert.match(
      result.stderr,
      /Timed out resolving the main integration reservation ref; inspect the ref for corruption/,
    );
    assert.equal(run('git', ['rev-parse', ref], f.repo), oid);
    assert.equal(existsSync(env.HS2_TEST_GATE_LOG), false);
    assert.equal(
      spawnSync('git', ['show-ref', '--exists', 'refs/heads/main'], {
        cwd: f.remote,
        env: withoutGitRepositoryEnv(process.env),
      }).status,
      2,
    );
  } finally {
    f.dispose();
  }
});

test('HS2-BYDMV4 owner verification cannot retry or grant authority to unresolved presence', () => {
  const f = fixture();
  try {
    const { oid, head, ref } = expiredRef(f),
      env = unresolvedRefEnvironment(f, true),
      module = new URL('./main-integration-window.mjs', import.meta.url).href,
      program = `
import { verifyMainPush } from ${JSON.stringify(module)};
try { verifyMainPush(${JSON.stringify(f.repo)}, 'synthetic-expired-owner', 'origin', 'refs/heads/main', ${JSON.stringify(head)}, ${JSON.stringify('0'.repeat(40))}); }
catch (error) { console.error(error.message); process.exitCode = 1; }
`;
    const result = spawnSync(process.execPath, ['--input-type=module', '-e', program], {
      cwd: f.repo,
      env,
      encoding: 'utf8',
      timeout: 15_000,
    });
    assert.ifError(result.error);
    assert.equal(result.status, 1);
    assert.equal(result.signal, null);
    assert.match(result.stderr, /Main integration ref exists but its reservation is unresolved/);
    assert.equal(run('git', ['rev-parse', ref], f.repo), oid);
    assert.equal(existsSync(env.HS2_TEST_GATE_LOG), false);
  } finally {
    f.dispose();
  }
});

test('HS2-BYDMV4 a clean live-owner read resets unresolved expiry to ordinary contention', () => {
  const f = fixture();
  try {
    const { oid: expiredOid, head, ref } = expiredRef(f),
      env = unresolvedRefEnvironment(f),
      module = new URL('./main-integration-window.mjs', import.meta.url).href,
      value = {
        version: 1,
        token: 'synthetic-live-owner',
        pid: process.pid,
        expiresAt: Date.now() + 20 * 60_000,
        phase: 'gating',
        remote: 'origin',
        target: 'refs/heads/main',
        head,
        tree: run('git', ['rev-parse', 'HEAD^{tree}'], f.repo),
      },
      oid = run('git', ['hash-object', '-w', '--stdin'], f.repo, env, JSON.stringify(value));
    run('git', ['update-ref', ref, oid, expiredOid], f.repo);
    const program = `
import { existsSync } from 'node:fs';
import { pushMain } from ${JSON.stringify(module)};
const originalNow = Date.now, started = originalNow();
// The second lookup is real and synchronous; advance only once that normal read is reached.
Date.now = () => existsSync(process.env.HS2_TEST_REF_RESOLVED) ? started + 10 * 60_000 + 1 : started;
try { await pushMain(${JSON.stringify(f.repo)}, []); }
catch (error) { console.error(error.message); process.exitCode = 1; }
finally { Date.now = originalNow; }
`;
    const result = spawnSync(process.execPath, ['--input-type=module', '-e', program], {
      cwd: f.repo,
      env,
      encoding: 'utf8',
      timeout: 15_000,
    });
    assert.ifError(result.error);
    assert.equal(result.status, 1);
    assert.equal(result.signal, null);
    assert.match(result.stderr, /Timed out waiting for the local main integration window/);
    assert.doesNotMatch(result.stderr, /Timed out resolving/);
    for (const marker of [env.HS2_TEST_REF_LOOKUP, env.HS2_TEST_REF_PRESENT, env.HS2_TEST_REF_RESOLVED])
      assert.equal(readFileSync(marker, 'utf8'), '');
    assert.equal(run('git', ['rev-parse', ref], f.repo), oid);
    assert.equal(existsSync(env.HS2_TEST_GATE_LOG), false);
    assert.equal(
      spawnSync('git', ['show-ref', '--exists', 'refs/heads/main'], {
        cwd: f.remote,
        env: withoutGitRepositoryEnv(process.env),
      }).status,
      2,
    );
  } finally {
    f.dispose();
  }
});

test('HS2-BYDMV4 a failed real CAS with unresolved reread stays inside acquisition retry', () => {
  const f = fixture();
  try {
    const { head, ref } = expiredRef(f),
      env = unresolvedRefEnvironment(f),
      module = new URL('./main-integration-window.mjs', import.meta.url).href,
      value = {
        version: 1,
        token: 'synthetic-competing-expired-owner',
        pid: process.pid,
        expiresAt: 0,
        phase: 'gating',
        remote: 'origin',
        target: 'refs/heads/main',
        head,
        tree: run('git', ['rev-parse', 'HEAD^{tree}'], f.repo),
      };
    env.HS2_TEST_REF_COMPETITOR = run('git', ['hash-object', '-w', '--stdin'], f.repo, env, JSON.stringify(value));
    env.HS2_TEST_REF_CAS_FAILURE = resolve(f.root, 'actual-cas-failure');
    const result = spawnSync(
      process.execPath,
      [
        '--input-type=module',
        '-e',
        `
import { pushMain } from ${JSON.stringify(module)};
await pushMain(${JSON.stringify(f.repo)}, []);
`,
      ],
      { cwd: f.repo, env, encoding: 'utf8', timeout: 15_000 },
    );
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.signal, null);
    assert.match(readFileSync(env.HS2_TEST_REF_CAS_FAILURE, 'utf8'), /^[1-9]\d*\n$/);
    assert.equal(readFileSync(env.HS2_TEST_REF_LOOKUP, 'utf8'), '');
    assert.equal(readFileSync(env.HS2_TEST_REF_PRESENT, 'utf8'), '');
    assert.equal(readFileSync(env.HS2_TEST_GATE_LOG, 'utf8'), 'gate\n');
    assert.equal(run('git', ['rev-parse', 'refs/heads/main'], f.remote), head);
    assert.equal(readWindow(f.repo), null);
    assert.throws(() => run('git', ['show-ref', '--verify', ref], f.repo));
  } finally {
    f.dispose();
  }
});

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

test('HS2-822EVF standalone push uses the linked store for pool authority', () => {
  const f = fixture();
  try {
    const store = resolve(f.root, 'tickets');
    mkdirSync(store);
    put(resolve(f.repo, '.gitignore'), '/.hotsheet2/store\n');
    run('git', ['add', '.gitignore'], f.repo, f.env);
    run('git', ['commit', '-m', 'Ignore local store link'], f.repo, f.env);
    put(resolve(f.repo, '.hotsheet2/store'), `${store}\n`);
    const canonical = realpathSync(f.repo);
    const state = JSON.stringify({
      version: 9,
      project_id: `repo-${createHash('sha256').update(canonical).digest('hex').slice(0, 12)}`,
      source_id: createHash('sha256').update(realpathSync(store)).digest('hex'),
      integrator: null,
      assignments: {},
    });
    put(resolve(f.repo, '.git/hotsheet-coordination/state.json'), state);
    run('bash', ['scripts/push.sh', '--standalone'], f.repo, f.env);
    assert.equal(run('git', ['rev-parse', 'refs/heads/main'], f.remote), run('git', ['rev-parse', 'HEAD'], f.repo));
  } finally {
    f.dispose();
  }
});

test('HS2-Q390N2 explicit standalone push skips local gate but verifies its reservation', () => {
  const f = fixture();
  try {
    put(f.env.HS2_TEST_FAIL, 'fail gate if it runs\n');
    const head = run('git', ['rev-parse', 'HEAD'], f.repo);
    run('bash', ['scripts/push.sh', '--standalone', '--skip-tests'], f.repo, f.env);
    assert.equal(run('git', ['rev-parse', 'refs/heads/main'], f.remote), head);
    assert.equal(existsSync(f.env.HS2_TEST_GATE_LOG), false);
    assert.equal(readWindow(f.repo), null);
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
    await waitForFile(f.env.HS2_TEST_STARTED, running);
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
    await waitForFile(f.env.HS2_TEST_STARTED, running);
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
    await waitForFile(f.env.HS2_TEST_STARTED, first);
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

test("HS2-Z333GJ accepts only this repository's .githooks, absolute or relative, from the checkout and its worktrees", () => {
  const root = realpathSync(mkdtempSync(resolve(tmpdir(), 'hs2-z333gj-')));
  try {
    const repo = resolve(root, 'repo'),
      worktree = resolve(root, 'linked'),
      env = {
        ...withoutGitRepositoryEnv(process.env),
        GIT_AUTHOR_NAME: 't',
        GIT_AUTHOR_EMAIL: 't@example.invalid',
        GIT_COMMITTER_NAME: 't',
        GIT_COMMITTER_EMAIL: 't@example.invalid',
      };
    mkdirSync(repo);
    run('git', ['init', '-q', '-b', 'main'], repo, env);
    put(resolve(repo, '.githooks/pre-push'), '#!/bin/sh\n', true);
    run('git', ['add', '.'], repo, env);
    run('git', ['commit', '-q', '-m', 'hooks'], repo, env);
    run('git', ['worktree', 'add', '-q', '--detach', worktree], repo, env);
    put(resolve(root, 'elsewhere/.githooks/pre-push'), '#!/bin/sh\n', true);
    const mainHooks = resolve(repo, '.githooks');
    // The installer's relative path, and the absolute path worktree creation leaves behind.
    for (const place of [repo, worktree]) {
      assert.equal(acceptedHooksPath(place, '.githooks'), true, `relative from ${place}`);
      assert.equal(acceptedHooksPath(place, mainHooks), true, `absolute main hooks from ${place}`);
      // Anything outside this repository's tracked hooks stays refused.
      assert.equal(acceptedHooksPath(place, resolve(root, 'elsewhere/.githooks')), false);
      assert.equal(acceptedHooksPath(place, resolve(root, 'missing')), false);
      assert.equal(acceptedHooksPath(place, resolve(repo, '.git/hooks')), false);
    }
    assert.equal(acceptedHooksPath(worktree, resolve(worktree, '.githooks')), true);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test(
  'HS2-RXYJ6G a heartbeat that loses the reservation stops the gate and names why',
  { timeout: 60_000 },
  async () => {
    const f = fixture();
    let running;
    const hold = resolve(f.root, 'hold');
    try {
      run('bash', ['scripts/push.sh'], f.repo, f.env);
      const remoteBase = run('git', ['rev-parse', 'refs/heads/main'], f.remote);
      commit(f.repo, 'heartbeat', f.env);
      put(hold, 'hold');
      running = runAsync('bash', ['scripts/push.sh'], f.repo, {
        ...f.env,
        HS2_TEST_HOLD: hold,
        HOTSHEET_MAIN_WINDOW_HEARTBEAT_MS: '100',
      });
      await waitForFile(f.env.HS2_TEST_STARTED, running);
      // Something outside this push removes the reservation while its gate runs.
      run('git', ['update-ref', '-d', 'refs/hotsheet/main-integration-window'], f.repo);
      // The gate is still held: only the heartbeat can end it.
      const result = await running.completed;
      assert.notEqual(result.code, 0, result.output);
      assert.match(result.output, /heartbeat stopped the push/);
      // HS2-RY0ZMX: the gate's own stop names this window and the heartbeat, not `failed (null)`.
      assert.match(result.output, /check\.sh was stopped by SIGTERM from this window because the heartbeat failed/);
      assert.match(result.output, /Gate processes running when it stopped:/);
      // The deletion can land before the ownership read or during the renewal's compare-and-swap.
      assert.match(result.output, /reservation ref is missing|changed during renewal/);
      assert.equal(run('git', ['rev-parse', 'refs/heads/main'], f.remote), remoteBase);
    } finally {
      rmSync(hold, { force: true });
      if (running?.child.exitCode === null)
        try {
          process.kill(-running.child.pid, 'SIGTERM');
        } catch {
          /* exited */
        }
      f.dispose();
    }
  },
);

test('HS2-822EVF a transient heartbeat ref read keeps the gate and push alive', { timeout: 60_000 }, async () => {
  const f = fixture();
  const hold = resolve(f.root, 'hold');
  const glitch = resolve(f.root, 'ref-read-glitch');
  const seen = resolve(f.root, 'ref-read-seen');
  let running;
  try {
    run('bash', ['scripts/push.sh'], f.repo, f.env);
    commit(f.repo, 'transient-ref-read', f.env);
    const head = run('git', ['rev-parse', 'HEAD'], f.repo, f.env);
    const realGit = run('sh', ['-c', 'command -v git'], f.repo, f.env);
    put(
      resolve(f.root, 'bin/git'),
      `#!/bin/sh
if [ "$1" = rev-parse ] && [ "$2" = --verify ] && [ "$3" = --quiet ] && [ "$4" = refs/hotsheet/main-integration-window ] && [ -f "$HS2_TEST_GLITCH" ]; then
  /bin/rm "$HS2_TEST_GLITCH"
  printf 'seen\\n' > "$HS2_TEST_GLITCH_SEEN"
  printf 'synthetic transient Git read failure\\n' >&2
  exit 128
fi
exec "${realGit}" "$@"
`,
      true,
    );
    put(hold, 'hold');
    running = runAsync('bash', ['scripts/push.sh'], f.repo, {
      ...f.env,
      HS2_TEST_HOLD: hold,
      HS2_TEST_GLITCH: glitch,
      HS2_TEST_GLITCH_SEEN: seen,
      HOTSHEET_MAIN_WINDOW_HEARTBEAT_MS: '500',
    });
    await waitForFile(f.env.HS2_TEST_STARTED, running);
    put(glitch, 'fail the next ref read');
    const deadline = Date.now() + 10_000;
    while (!existsSync(seen) && Date.now() < deadline) await new Promise((done) => setTimeout(done, 100));
    assert.equal(existsSync(seen), true, 'heartbeat encountered the injected Git read failure');
    rmSync(hold);
    const result = await running.completed;
    assert.equal(result.code, 0, result.output);
    assert.equal(run('git', ['rev-parse', 'refs/heads/main'], f.remote), head);
  } finally {
    rmSync(hold, { force: true });
    if (running?.child.exitCode === null)
      try {
        process.kill(-running.child.pid, 'SIGTERM');
      } catch {
        /* exited */
      }
    f.dispose();
  }
});

test('HS2-RY0ZMX gate diagnostics list other gates and windows, never this push, with the load', () => {
  const listing = [
    `${process.pid} 00:05 node scripts/main-integration-window.mjs push origin main`,
    '4101 12:30 node scripts/local-gate.mjs --ticket HS2-SYNTHETIC',
    '4102 01:02 bash scripts/check.sh',
    '4103 03:00 node unrelated.mjs',
    `4104 00:01 node scripts/local-gate.mjs ${'x'.repeat(300)}`,
  ].join('\n');
  const text = gateDiagnostics(() => ({ status: 0, stdout: `${listing}\n` }));
  const lines = text.split('\n');
  assert.equal(lines[0], 'Gate processes running when it stopped:');
  assert.ok(lines.includes('  4101 12:30 node scripts/local-gate.mjs --ticket HS2-SYNTHETIC'));
  assert.ok(lines.includes('  4102 01:02 bash scripts/check.sh'));
  assert.ok(!text.includes('unrelated.mjs'));
  assert.ok(!text.includes(`${process.pid} 00:05`), 'the push itself is not reported as another gate');
  assert.ok(lines.some((line) => line.startsWith('  4104 ') && line.length === 202 && line.endsWith('…')));
  assert.match(lines.at(-1), /^load average \(1, 5, 15 min\): \d+\.\d{2} \d+\.\d{2} \d+\.\d{2}$/);
  assert.match(
    gateDiagnostics(() => ({ status: 0, stdout: '4103 03:00 node unrelated.mjs\n' })),
    /stopped:\n {2}none\n/,
  );
  assert.match(
    gateDiagnostics(() => ({ status: 1, stdout: '' })),
    /\(process list unavailable\)/,
  );
});

/** The held gate's pid, which also leads its process group (`child()` spawns it detached). */
async function heldGatePid(f, running) {
  await waitForFile(f.env.HS2_TEST_STARTED, running);
  for (;;) {
    const pid = Number(readFileSync(f.env.HS2_TEST_STARTED, 'utf8').trim());
    if (pid > 0) return pid;
    await new Promise((done) => setTimeout(done, 50));
  }
}

for (const signal of ['SIGTERM', 'SIGINT'])
  test(
    `HS2-RY0ZMX a ${signal} to the push names itself as the reason the gate stopped`,
    { timeout: 60_000 },
    async () => {
      const f = fixture();
      let running;
      const hold = resolve(f.root, 'hold');
      try {
        run('bash', ['scripts/push.sh'], f.repo, f.env);
        const remoteBase = run('git', ['rev-parse', 'refs/heads/main'], f.remote);
        commit(f.repo, `signal-${signal}`, f.env);
        put(hold, 'hold');
        running = runAsync('bash', ['scripts/push.sh'], f.repo, { ...f.env, HS2_TEST_HOLD: hold });
        await heldGatePid(f, running);
        // An agent tool ending a long foreground command signals the push's own group, not the
        // separately detached gate group: only the push's handler can stop the gate.
        process.kill(-running.child.pid, signal);
        const result = await running.completed;
        assert.notEqual(result.code, 0, result.output);
        assert.match(result.output, new RegExp(`push received ${signal} and stopped its gate`));
        assert.match(result.output, /bash scripts\/check\.sh was stopped by SIGTERM from this window/);
        assert.doesNotMatch(result.output, /failed \(null\)/);
        assert.equal(run('git', ['rev-parse', 'refs/heads/main'], f.remote), remoteBase);
        assert.equal(
          spawnSync('git', ['show-ref', '--exists', 'refs/hotsheet/main-integration-window'], {
            cwd: f.repo,
            env: withoutGitRepositoryEnv(process.env),
          }).status,
          2,
          'the stopped push releases its reservation',
        );
      } finally {
        rmSync(hold, { force: true });
        if (running?.child.exitCode === null)
          try {
            process.kill(-running.child.pid, 'SIGKILL');
          } catch {
            /* exited */
          }
        f.dispose();
      }
    },
  );

test(
  'HS2-RY0ZMX a gate killed from outside the window is reported as an external signal with running gates',
  { timeout: 60_000 },
  async () => {
    const f = fixture();
    let running;
    const hold = resolve(f.root, 'hold');
    try {
      run('bash', ['scripts/push.sh'], f.repo, f.env);
      const remoteBase = run('git', ['rev-parse', 'refs/heads/main'], f.remote);
      commit(f.repo, 'external', f.env);
      put(hold, 'hold');
      running = runAsync('bash', ['scripts/push.sh'], f.repo, { ...f.env, HS2_TEST_HOLD: hold });
      const gate = await heldGatePid(f, running);
      process.kill(-gate, 'SIGKILL');
      const result = await running.completed;
      assert.notEqual(result.code, 0, result.output);
      assert.match(result.output, /bash scripts\/check\.sh was stopped by SIGKILL from outside this window/);
      assert.doesNotMatch(result.output, /heartbeat stopped the push|push received/);
      // The diagnostic names what else was running so the next occurrence can be attributed.
      assert.match(result.output, /Gate processes running when it stopped:/);
      assert.match(result.output, /load average/);
      assert.equal(run('git', ['rev-parse', 'refs/heads/main'], f.remote), remoteBase);
    } finally {
      rmSync(hold, { force: true });
      if (running?.child.exitCode === null)
        try {
          process.kill(-running.child.pid, 'SIGKILL');
        } catch {
          /* exited */
        }
      f.dispose();
    }
  },
);

// HS2-GT57BQ: a live ci-staged pool and its batch ledger in the fixture's coordination directory.
function stagedPool(f, ledger) {
  const canonical = realpathSync(f.repo);
  const directory = resolve(f.repo, '.git/hotsheet-coordination');
  put(
    resolve(directory, 'state.json'),
    JSON.stringify({
      version: 8,
      project_id: `repo-${createHash('sha256').update(canonical).digest('hex').slice(0, 12)}`,
      source_id: createHash('sha256').update(canonical).digest('hex'),
      stopped: null,
      integrator: { id: 'i', generation: 'g', expires_at: Math.floor(Date.now() / 1000) + 600, strategy: 'ci-staged' },
      assignments: {},
    }),
  );
  put(resolve(directory, 'batches.json'), JSON.stringify(ledger));
}

const stagedLedger = (state, sha, tripped = false) => ({
  version: 2,
  batches: { 'batch-1': { id: 'batch-1', state, staging: { branch: 'integration', staged_commit: sha } } },
  breaker: tripped ? { consecutive_red: 2, tripped: { at: 1, reason: 'synthetic' } } : { consecutive_red: 0 },
});

const integrator = ['--integrator-id', 'i', '--integrator-session', 'g'];

test('HS2-GT57BQ staged pushes move only the integration branch and promotion fast-forwards main exactly', () => {
  const f = fixture();
  try {
    const env = withoutGitRepositoryEnv(f.env);
    run('bash', ['scripts/push.sh'], f.repo, env);
    const main = run('git', ['rev-parse', 'refs/heads/main'], f.remote);
    stagedPool(f, stagedLedger('ci', '0'.repeat(40)));
    run('git', ['switch', '-qc', 'integration'], f.repo, env);
    commit(f.repo, 'staged-1', env);
    const first = run('git', ['rev-parse', 'HEAD'], f.repo);
    run('bash', ['scripts/push.sh', '--staged', ...integrator, '--gate-arg', '--smoke'], f.repo, env);
    assert.equal(run('git', ['rev-parse', 'refs/heads/integration'], f.remote), first);
    assert.equal(run('git', ['rev-parse', 'refs/heads/main'], f.remote), main);
    // The hook verified the reservation instead of running the root gate a second time.
    assert.equal(readFileSync(f.env.HS2_TEST_GATE_LOG, 'utf8'), 'gate\ngate\n');
    // A ci-staged integrator never pushes main directly unless its breaker tripped.
    assert.throws(() => run('bash', ['scripts/push.sh', ...integrator], f.repo, env), /only through --promote/);
    // A rebuilt tip replaces the unpromoted old tip, never rewriting main.
    run('git', ['reset', '-q', '--hard', 'main'], f.repo, env);
    commit(f.repo, 'staged-2', env);
    const rebuilt = run('git', ['rev-parse', 'HEAD'], f.repo);
    run('bash', ['scripts/push.sh', '--staged', ...integrator, '--gate-arg', '--smoke'], f.repo, env);
    assert.equal(run('git', ['rev-parse', 'refs/heads/integration'], f.remote), rebuilt);
    // Promotion needs a green ledger batch for the exact SHA and runs no local gate.
    assert.throws(() => run('bash', ['scripts/push.sh', '--promote', rebuilt, ...integrator], f.repo, env), /No green/);
    stagedPool(f, stagedLedger('green', rebuilt));
    run('git', ['switch', '-q', 'main'], f.repo, env);
    run('bash', ['scripts/push.sh', '--promote', rebuilt, ...integrator], f.repo, env);
    assert.equal(run('git', ['rev-parse', 'refs/heads/main'], f.remote), rebuilt);
    assert.equal(readFileSync(f.env.HS2_TEST_GATE_LOG, 'utf8'), 'gate\ngate\ngate\n');
    // Replaying a completed promotion pushes nothing.
    run('bash', ['scripts/push.sh', '--promote', rebuilt, ...integrator], f.repo, env);
    assert.equal(run('git', ['rev-parse', 'refs/heads/main'], f.remote), rebuilt);
  } finally {
    f.dispose();
  }
});

test('HS2-GT57BQ promotion is refused once main moved and no longer fast-forwards', () => {
  const f = fixture();
  try {
    const env = withoutGitRepositoryEnv(f.env);
    run('bash', ['scripts/push.sh'], f.repo, env);
    run('git', ['switch', '-qc', 'integration'], f.repo, env);
    commit(f.repo, 'staged', env);
    const staged = run('git', ['rev-parse', 'HEAD'], f.repo);
    // Another writer moved main after the batch was staged.
    const writer = resolve(f.root, 'writer');
    run('git', ['clone', '-q', f.remote, writer], f.root, env);
    run('git', ['config', 'user.name', 'Remote Test'], writer);
    run('git', ['config', 'user.email', 'remote@invalid.example'], writer);
    commit(writer, 'moved', env);
    run('git', ['push', '-q', 'origin', 'main'], writer, env);
    const moved = run('git', ['rev-parse', 'refs/heads/main'], f.remote);
    stagedPool(f, stagedLedger('green', staged));
    assert.throws(
      () => run('bash', ['scripts/push.sh', '--promote', staged, ...integrator], f.repo, env),
      /no longer fast-forwards/,
    );
    assert.equal(run('git', ['rev-parse', 'refs/heads/main'], f.remote), moved);
    // While the breaker is tripped, the integrator may integrate serially onto main.
    stagedPool(f, stagedLedger('green', staged, true));
    run('git', ['switch', '-q', 'main'], f.repo, env);
    run('git', ['pull', '-q', '--ff-only', 'origin', 'main'], f.repo, env);
    commit(f.repo, 'serial', env);
    run('bash', ['scripts/push.sh', ...integrator], f.repo, env);
    assert.equal(run('git', ['rev-parse', 'refs/heads/main'], f.remote), run('git', ['rev-parse', 'HEAD'], f.repo));
  } finally {
    f.dispose();
  }
});
