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
import { readWindow, verifyMainPush } from './main-integration-window.mjs';

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
        const result = spawnSync('git', args, { cwd: repo, encoding: 'utf8' });
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
          const result = spawnSync('git', args, { cwd: repo, encoding: 'utf8' });
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
    assert.equal(spawnSync('git', ['show-ref', '--exists', 'refs/heads/main'], { cwd: f.remote }).status, 2);
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
    assert.equal(spawnSync('git', ['show-ref', '--exists', 'refs/heads/main'], { cwd: f.remote }).status, 2);
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
