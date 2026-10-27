import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

import {
  candidateArtifacts,
  crtRequests,
  identity,
  main,
  memberIdentity,
  parseDirectives,
  summarize,
} from './windows-crt-directives.mjs';

// Synthetic dumpbin output shaped like `/ARCHIVEMEMBERS /DIRECTIVES` on a CMake Debug archive and
// on a Rust rlib whose LLVM objects use `-defaultlib:` forms. No real build paths are copied.
const ARCHIVE = `
Dump of file C:\\work\\target\\debug\\build\\libavif-sys-1\\out\\lib\\avif.lib

File Type: LIBRARY

Archive member name at 8: /
Archive member name at 4F2: //
Archive member name at 6A0: /0              C:\\work\\target\\debug\\build\\libavif-sys-1\\out\\build\\avif.dir\\Debug\\read.obj

   Linker Directives
   -----------------
   /FAILIFMISMATCH:_MSC_VER=1900
   /FAILIFMISMATCH:RuntimeLibrary=MDd_DynamicDebug
   /DEFAULTLIB:msvcprtd
   /DEFAULTLIB:MSVCRTD
   /DEFAULTLIB:OLDNAMES

Archive member name at 9B0: /42             C:\\work\\target\\debug\\build\\libavif-sys-1\\out\\build\\avif.dir\\Debug\\write.obj

   Linker Directives
   -----------------
   /DEFAULTLIB:MSVCRTD.lib
`;

const RLIB = `
Archive member name at 100: lib.rmeta/
Archive member name at 200: sqlite3.o/

   Linker Directives
   -----------------
   -defaultlib:"msvcrt"
   -defaultlib:"oldnames"
`;

test('HS2-1R2RGQ parses per-member default libraries and runtime mismatch tags', () => {
  const members = parseDirectives(ARCHIVE);
  assert.deepEqual(
    members.map((member) => [member.member.split('\\').at(-1), member.defaultlibs, member.runtimeLibrary]),
    [
      ['read.obj', ['MSVCPRTD', 'MSVCRTD', 'OLDNAMES'], ['MDd_DynamicDebug']],
      ['write.obj', ['MSVCRTD'], []],
    ],
  );
  assert.deepEqual(
    crtRequests(members).map((request) => request.crt),
    [['MSVCRTD'], ['MSVCRTD']],
  );
  assert.deepEqual(crtRequests(parseDirectives(RLIB)), [{ member: 'sqlite3.o/', crt: ['MSVCRT'], runtimeLibrary: [] }]);
  // A plain object without member headers attributes directives to the artifact itself.
  assert.deepEqual(parseDirectives('   /DEFAULTLIB:LIBCMT\n', 'dav1d.lib'), [
    { member: 'dav1d.lib', defaultlibs: ['LIBCMT'], runtimeLibrary: [] },
  ]);
  assert.deepEqual(parseDirectives('File Type: LIBRARY\n\n  Summary\n'), []);
});

test('HS2-1R2RGQ reports only relative artifact identities', () => {
  const roots = [
    ['target', 'C:\\work\\target'],
    ['vcpkg', 'C:\\vcpkg'],
  ];
  assert.equal(
    identity('/0              C:\\work\\target\\debug\\build\\x\\out\\read.obj', roots),
    'target/debug/build/x/out/read.obj',
  );
  assert.equal(
    identity('C:\\vcpkg\\installed\\x64-windows-static-md\\lib\\dav1d.lib', roots),
    'vcpkg/installed/x64-windows-static-md/lib/dav1d.lib',
  );
  assert.equal(identity('D:\\elsewhere\\private\\thing.obj', roots), 'thing.obj');
  assert.equal(identity('sqlite3.o/', roots), 'sqlite3.o');
  assert.equal(identity('C:\\work\\target\\', roots), 'target');
});

test('HS2-9E879K preserves complete private member identity before display sanitization', () => {
  const first = memberIdentity('/0     C:\\build\\a\\thread.obj');
  assert.match(first.sha256, /^[a-f0-9]{64}$/);
  assert.equal(first.version, 1);
  assert.deepEqual(first, memberIdentity('c:/BUILD/a/obj/../THREAD.OBJ'));
  assert.notDeepEqual(first, memberIdentity('C:/build/b/thread.obj'));
  assert.notDeepEqual(first, memberIdentity('thread.obj'));
  assert.doesNotMatch(JSON.stringify(first), /build|thread|C:/);
});

test('HS2-1R2RGQ bounds cached candidates to native outputs and the selected vcpkg triplet', () => {
  const root = mkdtempSync(join(tmpdir(), 'hs2-1r2rgq-'));
  try {
    const target = join(root, 'target');
    const vcpkg = join(root, 'vcpkg');
    const files = [
      'target/debug/build/libavif-sys-1/out/lib/avif.lib',
      'target/debug/build/libavif-sys-1/out/build/notes.txt',
      'target/debug/build/libsqlite3-sys-2/out/sqlite3.lib',
      'target/debug/deps/liblibavif_sys-0123abcd.rlib',
      'target/debug/deps/libserde-0123abcd.rlib',
      'target/release/build/other/out/ignored.lib',
      'vcpkg/installed/x64-windows-static-md/lib/dav1d.lib',
      'vcpkg/installed/x64-windows-static-md/debug/lib/dav1d.lib',
      'vcpkg/installed/x64-windows-static-md/lib/zlib.lib',
      'vcpkg/installed/x64-windows/lib/dav1d.lib',
    ];
    for (const file of files) {
      mkdirSync(join(root, file, '..'), { recursive: true });
      writeFileSync(join(root, file), '');
    }
    const found = candidateArtifacts({ target, vcpkgRoot: vcpkg }).map(({ kind, path }) => [
      kind,
      path.slice(root.length + 1).replaceAll('\\', '/'),
    ]);
    assert.deepEqual(found, [
      ['build-script-lib', 'target/debug/build/libavif-sys-1/out/lib/avif.lib'],
      ['build-script-lib', 'target/debug/build/libsqlite3-sys-2/out/sqlite3.lib'],
      ['sys-rlib', 'target/debug/deps/liblibavif_sys-0123abcd.rlib'],
      ['vcpkg-debug', 'vcpkg/installed/x64-windows-static-md/debug/lib/dav1d.lib'],
      ['vcpkg-release', 'vcpkg/installed/x64-windows-static-md/lib/dav1d.lib'],
    ]);
    assert.deepEqual(candidateArtifacts({ target: join(root, 'missing') }), []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('HS2-1R2RGQ summarizes requesters per C runtime', () => {
  const summary = summarize([
    {
      artifact: 'target/a.lib',
      kind: 'build-script-lib',
      requests: [{ member: 'read.obj', crt: ['MSVCRTD'], runtimeLibrary: ['MDd_DynamicDebug'] }],
    },
    {
      artifact: 'target/b.rlib',
      kind: 'sys-rlib',
      requests: [{ member: 'sqlite3.o', crt: ['MSVCRT'], runtimeLibrary: [] }],
    },
    { artifact: 'target/c.lib', kind: 'build-script-lib', error: 'exit 1' },
  ]);
  assert.equal(summary.scanned, 3);
  assert.equal(summary.failed, 1);
  assert.deepEqual(summary.counts, { MSVCRT: 1, MSVCRTD: 1, LIBCMT: 0, LIBCMTD: 0 });
  assert.deepEqual(summary.requesters.MSVCRTD, [
    { artifact: 'target/a.lib', kind: 'build-script-lib', member: 'read.obj', runtimeLibrary: ['MDd_DynamicDebug'] },
  ]);
});

function fixtureRepo(withArtifact = true) {
  const repo = mkdtempSync(join(tmpdir(), 'hs2-1r2rgq-run-'));
  if (withArtifact) {
    mkdirSync(join(repo, 'target/debug/build/libavif-sys-1/out'), { recursive: true });
    writeFileSync(join(repo, 'target/debug/build/libavif-sys-1/out/avif.lib'), 'synthetic');
  }
  return repo;
}

test('HS2-1R2RGQ fails explicitly without dumpbin or without artifacts', () => {
  const lines = [];
  const repo = fixtureRepo(false);
  try {
    const missingTool = main({
      argv: ['--repo', repo],
      env: {},
      log: (line) => lines.push(line),
      dumpbin: () => ({ error: Object.assign(new Error('spawn dumpbin ENOENT'), { code: 'ENOENT' }) }),
    });
    assert.equal(missingTool, 2);
    assert.match(lines.at(-1), /dumpbin is unavailable \(ENOENT\); no CRT evidence was collected/);
    const noArtifacts = main({
      argv: ['--repo', repo],
      env: {},
      log: (line) => lines.push(line),
      dumpbin: () => ({ status: 0, stdout: '' }),
    });
    assert.equal(noArtifacts, 3);
    assert.match(lines.at(-1), /no native build artifacts were found under target; no CRT evidence was collected/);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test('HS2-1R2RGQ reports requesting members and fails when every artifact is unreadable', () => {
  const repo = fixtureRepo();
  try {
    const lines = [];
    const ok = main({
      argv: ['--repo', repo],
      env: {},
      log: (line) => lines.push(line),
      dumpbin: (args) => (args.length === 1 ? { status: 0, stdout: '' } : { status: 0, stdout: ARCHIVE }),
    });
    assert.equal(ok, 0);
    assert.ok(lines.some((line) => line.includes('MSVCRTD=2')));
    assert.ok(
      lines.some((line) =>
        /^candidate MSVCRTD <- target\/debug\/build\/libavif-sys-1\/out\/avif\.lib \[build-script-lib\] member read\.obj RuntimeLibrary=MDd_DynamicDebug$/.test(
          line,
        ),
      ),
    );
    const report = JSON.parse(readFileSync(join(repo, 'target/crt-directives/report.json'), 'utf8'));
    assert.equal(report.ticket, 'HS2-1R2RGQ');
    assert.equal(report.scanned, 1);
    assert.deepEqual(report.requesters.MSVCRTD[0].memberIdentity, memberIdentity(parseDirectives(ARCHIVE)[0].member));
    assert.deepEqual(report.results[0].requests[0].memberIdentity, report.requesters.MSVCRTD[0].memberIdentity);
    assert.doesNotMatch(JSON.stringify(report), new RegExp(repo.replaceAll('\\', '\\\\')), 'no machine-local paths');
    const unreadable = main({
      argv: ['--repo', repo],
      env: {},
      log: (line) => lines.push(line),
      dumpbin: (args) => (args.length === 1 ? { status: 0, stdout: '' } : { status: 1, stdout: '' }),
    });
    assert.equal(unreadable, 4);
    assert.match(lines.at(-1), /dumpbin failed on every scanned artifact/);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test('HS2-1R2RGQ wires the diagnostic after every native Windows step without masking', () => {
  const workflow = readFileSync(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8');
  const job = workflow.slice(workflow.indexOf('  windows-credential-store:'), workflow.indexOf('\n  check:'));
  const steps = [
    'cargo test -p hotsheet-ticketing windows_credential_manager_round_trip -- --ignored',
    'cargo test -p hotsheet-aitools permission_vault::tests',
    'cargo test -p hotsheet-aitools --test antigravity_discovery',
    'cargo test -p hotsheet-ticketing --test file_lock_process',
    'cargo test -p hotsheet-cli --test coordination --test coordination_portable --test coordination_dispatcher',
    'cargo test -p hotsheet-cli --test claim_run',
    'node scripts/windows-crt-directives.mjs --target target --output target/crt-directives/report.json',
    'path: target/crt-directives/report.json',
  ];
  const positions = steps.map((step) => job.indexOf(step));
  assert.ok(
    positions.every((position) => position > 0),
    'every native step and the diagnostic remain in the job',
  );
  assert.deepEqual(
    [...positions].sort((left, right) => left - right),
    positions,
    'the diagnostic runs last',
  );
  // Only this diagnostic's own two steps; the later HS2-9E879K correlation pins its own steps.
  const start = job.indexOf('Report native CRT directives on Windows'),
    next = job.indexOf('Correlate CRT candidates with one current affected link');
  const diagnostic = job.slice(start, next > start ? next : undefined);
  assert.equal(diagnostic.match(/if: \$\{\{ !cancelled\(\) \}\}/g)?.length, 2);
  assert.match(diagnostic, /if-no-files-found: error/);
  assert.doesNotMatch(workflow, /NODEFAULTLIB/i);
});

for (const failure of ['dumpbin_unavailable', 'no_candidates'])
  test(`HS2-1R2RGQ replaces a cached report when ${failure}`, () => {
    const repo = fixtureRepo(false);
    try {
      const output = join(repo, 'target/crt-directives/report.json');
      mkdirSync(join(output, '..'), { recursive: true });
      writeFileSync(output, JSON.stringify({ previousRunEvidence: true, scanned: 99 }));
      const status = main({
        argv: ['--repo', repo],
        env: { GITHUB_SHA: 'a'.repeat(40), GITHUB_RUN_ID: '123', GITHUB_RUN_ATTEMPT: '2' },
        log: () => {},
        dumpbin: () =>
          failure === 'dumpbin_unavailable'
            ? { error: Object.assign(new Error('missing'), { code: 'ENOENT' }) }
            : { status: 0, stdout: '' },
      });
      assert.equal(status, failure === 'dumpbin_unavailable' ? 2 : 3);
      const report = JSON.parse(readFileSync(output, 'utf8'));
      assert.equal(report.status, 'failed');
      assert.equal(report.failure, failure);
      assert.equal(report.previousRunEvidence, undefined);
      assert.equal(report.scanned, 0);
      assert.deepEqual(report.provenance, {
        sourceSha: 'a'.repeat(40),
        runId: '123',
        runAttempt: '2',
        unavailable: [],
      });
    } finally {
      rmSync(repo, { recursive: true, force: true });
    }
  });

test('HS2-1R2RGQ actual entry point replaces cached evidence before a missing-tool exit', () => {
  const repo = fixtureRepo(false);
  try {
    const output = join(repo, 'target/crt-directives/report.json');
    const emptyPath = join(repo, 'empty-path');
    mkdirSync(join(output, '..'), { recursive: true });
    mkdirSync(emptyPath);
    writeFileSync(output, JSON.stringify({ previousRunEvidence: true }));
    const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => key.toLowerCase() !== 'path'));
    env.PATH = emptyPath;
    const result = spawnSync(
      process.execPath,
      [fileURLToPath(new URL('./windows-crt-directives.mjs', import.meta.url)), '--repo', repo],
      { env, encoding: 'utf8' },
    );
    assert.equal(result.status, 2, result.stderr);
    const report = JSON.parse(readFileSync(output, 'utf8'));
    assert.equal(report.status, 'failed');
    assert.equal(report.failure, 'dumpbin_unavailable');
    assert.equal(report.previousRunEvidence, undefined);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test('HS2-1R2RGQ labels cached candidates and never copies unvalidated provenance', () => {
  const repo = fixtureRepo();
  try {
    const lines = [];
    const status = main({
      argv: ['--repo', repo],
      env: {
        GITHUB_SHA: 'private/source',
        GITHUB_RUN_ID: 'private/run',
        GITHUB_RUN_ATTEMPT: '',
        UNRELATED_SECRET: 'synthetic-secret-marker',
      },
      log: (line) => lines.push(line),
      dumpbin: (args) => (args.length === 1 ? { status: 0, stdout: '' } : { status: 0, stdout: ARCHIVE }),
    });
    assert.equal(status, 0);
    const report = JSON.parse(readFileSync(join(repo, 'target/crt-directives/report.json'), 'utf8'));
    assert.equal(report.status, 'completed');
    assert.equal(report.selection, 'cached_artifact_candidates');
    assert.equal(report.currentLinkConfirmed, false);
    assert.deepEqual(report.provenance, {
      sourceSha: null,
      runId: null,
      runAttempt: null,
      unavailable: ['sourceSha', 'runId', 'runAttempt'],
    });
    assert.match(report.nextStep, /current.*link/i);
    assert.ok(lines.some((line) => /candidate/i.test(line)));
    assert.doesNotMatch(JSON.stringify(report), /private\/|synthetic-secret-marker/);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});
