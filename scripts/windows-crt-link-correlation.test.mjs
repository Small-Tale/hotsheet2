import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { correlate, LIMITS, linkCommand, linkerText, main, parseVerboseLink } from './windows-crt-link-correlation.mjs';
import { memberIdentity } from './windows-crt-directives.mjs';

// HS2-9E879K: synthetic fixtures only. Paths are invented Windows-shaped strings; no real build log,
// machine path or credential is copied.
const SHA = 'a'.repeat(40);
const VCPKG = 'C:\\vcpkg';
const DEBUG_DAV1D = `${VCPKG}\\installed\\x64-windows-static-md\\debug\\lib\\dav1d.lib`;
const RELEASE_DAV1D = `${VCPKG}\\installed\\x64-windows-static-md\\lib\\dav1d.lib`;
const VERBOSE = [
  'Searching libraries',
  `    Searching ${RELEASE_DAV1D}:`,
  `    Searching ${DEBUG_DAV1D}:`,
  '      Found dav1d_open',
  '        Referenced in avif.lib(read.obj)',
  '        Loaded dav1d.lib(C:\\build\\dav1d\\src\\lib.c.obj)',
  '      Found dav1d_picture_unref',
  `        Loaded ${DEBUG_DAV1D}(C:\\build\\dav1d\\src\\picture.c.obj)`,
  '    Searching C:\\Windows\\SDK\\kernel32.lib:',
  '      Processed /DEFAULTLIB:MSVCRTD',
  '      Processed /DEFAULTLIB:msvcrt.lib',
  "LINK : warning LNK4098: defaultlib 'MSVCRTD' conflicts with use of other libs; use /NODEFAULTLIB:library",
].join('\r\n');
const PACKAGE_ID = 'path+file:///synthetic/ticketing#hotsheet-ticketing@0.1.0';
const TARGET = {
  name: 'file_lock_process',
  kind: ['test'],
  src_path: '/synthetic/ticketing/tests/file_lock_process.rs',
};
const owner = { package_id: PACKAGE_ID, target: TARGET };
const event = (message, children = []) =>
  JSON.stringify({ reason: 'compiler-message', ...owner, message: { message, level: 'warning', children } });
const artifact = (fresh = false, identity = owner) =>
  JSON.stringify({ reason: 'compiler-artifact', ...identity, fresh });
const cargoStream = (text) =>
  [
    artifact(),
    event('unused variable: `x`'),
    'not json',
    '{"reason": broken',
    event(`linker stdout: ${text}`),
    JSON.stringify({ reason: 'build-finished', success: true }),
  ].join('\n');

test('HS2-9E879K refuses mixed target, mixed link, cached and missing Cargo attribution', (t) => {
  const verbose = event(`linker stdout: ${VERBOSE}`),
    other = {
      package_id: 'path+file:///synthetic#other@0.1.0',
      target: { name: 'other', kind: ['test'], src_path: '/synthetic/other.rs' },
    },
    missing = JSON.parse(verbose);
  delete missing.package_id;
  delete missing.target;
  for (const stdout of [
    JSON.stringify(missing),
    [artifact(), JSON.stringify({ ...JSON.parse(verbose), ...other })].join('\n'),
    [artifact(), JSON.stringify({ ...JSON.parse(verbose), package_id: other.package_id })].join('\n'),
    [
      artifact(),
      event(`linker stdout: Loaded ${DEBUG_DAV1D}(C:\\build\\dav1d\\src\\lib.c.obj)`),
      event("linker stdout: LINK : warning LNK4098: defaultlib 'MSVCRTD' conflicts"),
    ].join('\n'),
    [artifact(true), verbose].join('\n'),
    [artifact(false, other), verbose].join('\n'),
    [artifact(), artifact(), verbose].join('\n'),
    [
      artifact(),
      JSON.stringify({ ...JSON.parse(verbose), target: { ...TARGET, src_path: '/synthetic/other.rs' } }),
    ].join('\n'),
  ]) {
    const result = fixture(t, { stdout, previous: { currentLinkConfirmed: true, oldEvidence: true } });
    assert.equal(result.report.currentLinkConfirmed, false, 'unattributed/mixed evidence cannot confirm');
    assert.equal(result.report.currentLinkMatches.total, 0);
    assert.equal(result.report.oldEvidence, undefined);
  }
});
const directiveReport = (overrides = {}) => ({
  ticket: 'HS2-1R2RGQ',
  status: 'completed',
  provenance: { sourceSha: SHA, runId: '9', runAttempt: '2', unavailable: [] },
  requesters: {
    MSVCRTD: [
      {
        artifact: 'vcpkg/installed/x64-windows-static-md/debug/lib/dav1d.lib',
        kind: 'vcpkg-debug',
        member: 'lib.c.obj',
        memberIdentity: memberIdentity('C:\\build\\dav1d\\src\\lib.c.obj'),
        runtimeLibrary: ['MDd_DynamicDebug'],
      },
      {
        artifact: 'vcpkg/installed/x64-windows-static-md/debug/lib/dav1d.lib',
        kind: 'vcpkg-debug',
        member: 'picture.c.obj',
        memberIdentity: memberIdentity('C:\\build\\dav1d\\src\\picture.c.obj'),
        runtimeLibrary: [],
      },
      { artifact: 'target/debug/build/libavif-sys-1/out/lib/avif.lib', kind: 'build-script-lib', member: 'write.obj' },
    ],
  },
  ...overrides,
});

function fixture(
  t,
  {
    directives = directiveReport(),
    stdout = cargoStream(VERBOSE),
    status = 0,
    error,
    previous,
    env = {},
    metadata,
  } = {},
) {
  const repo = mkdtempSync(join(tmpdir(), 'hs2-9e879k-'));
  t.after(() => rmSync(repo, { recursive: true, force: true }));
  mkdirSync(join(repo, 'target', 'crt-directives'), { recursive: true });
  if (directives !== null)
    writeFileSync(
      join(repo, 'target', 'crt-directives', 'report.json'),
      typeof directives === 'string' ? directives : JSON.stringify(directives),
    );
  const output = join(repo, 'target', 'crt-directives', 'link-correlation.json');
  if (previous) writeFileSync(output, JSON.stringify(previous));
  const calls = [];
  const logs = [];
  const code = main({
    argv: ['--repo', repo],
    env: { GITHUB_SHA: SHA, GITHUB_RUN_ID: '9', GITHUB_RUN_ATTEMPT: '2', VCPKG_INSTALLATION_ROOT: VCPKG, ...env },
    log: (line) => logs.push(line),
    cargo: (args) => {
      if (args[0] === 'metadata') {
        assert.deepEqual(args, ['metadata', '--no-deps', '--format-version=1', '--offline']);
        return (
          metadata ?? {
            status: 0,
            stdout: JSON.stringify({ packages: [{ name: 'hotsheet-ticketing', id: PACKAGE_ID, targets: [TARGET] }] }),
          }
        );
      }
      calls.push(args);
      return error ? { error: { code: 'ENOENT' } } : { status, stdout };
    },
  });
  return {
    code,
    calls,
    logs,
    output,
    report: existsSync(output) ? JSON.parse(readFileSync(output, 'utf8')) : undefined,
  };
}

test('HS2-9E879K relinks exactly one owned test target with verbose linker output and no CRT flags', () => {
  const args = linkCommand({ packageName: 'hotsheet-ticketing', testName: 'file_lock_process' });
  assert.deepEqual(args, [
    'rustc',
    '-p',
    'hotsheet-ticketing',
    '--test',
    'file_lock_process',
    '--message-format=json',
    '--',
    '-C',
    'link-arg=/VERBOSE',
    '-W',
    'linker_messages',
  ]);
  assert.ok(!args.some((arg) => /NODEFAULTLIB|\/M[DT]d?\b|crt-static/i.test(arg)), 'no CRT or suppression flag');
});

test('HS2-9E879K reads only linker diagnostics from Cargo JSON and parses selections and LNK4098', () => {
  const text = linkerText(cargoStream(VERBOSE));
  assert.ok(!text.includes('unused variable'));
  const parsed = parseVerboseLink(text);
  assert.deepEqual(parsed.searched, [RELEASE_DAV1D, DEBUG_DAV1D, 'C:\\Windows\\SDK\\kernel32.lib']);
  // A bare archive name resolves to the searched path whose block selected it.
  assert.deepEqual(parsed.loaded, [
    { archive: DEBUG_DAV1D, member: 'C:\\build\\dav1d\\src\\lib.c.obj' },
    { archive: DEBUG_DAV1D, member: 'C:\\build\\dav1d\\src\\picture.c.obj' },
  ]);
  assert.deepEqual(parsed.defaultlibs, ['MSVCRTD', 'MSVCRT']);
  assert.deepEqual(parsed.lnk4098, { MSVCRTD: 1 });
});

test('HS2-9E879K correlates exact, basename and ambiguous candidate matches', () => {
  const debug = {
    library: 'MSVCRTD',
    artifact: 'vcpkg/debug/lib/dav1d.lib',
    kind: 'vcpkg-debug',
    member: 'lib.c.obj',
    memberIdentity: memberIdentity('lib.c.obj'),
  };
  const release = { ...debug, artifact: 'vcpkg/lib/dav1d.lib', kind: 'vcpkg-release' };
  assert.deepEqual(
    correlate(
      [debug],
      [{ archive: 'vcpkg/debug/lib/dav1d.lib', member: 'lib.c.obj', memberIdentity: memberIdentity('lib.c.obj') }],
    ).map((match) => match.match),
    ['exact'],
  );
  assert.deepEqual(
    correlate([debug], [{ archive: 'dav1d.lib', member: 'lib.c.obj' }]).map((match) => match.match),
    ['basename'],
  );
  assert.deepEqual(
    correlate([debug, release], [{ archive: 'dav1d.lib', member: 'lib.c.obj' }]).map((match) => match.match),
    ['ambiguous', 'ambiguous'],
  );
  assert.deepEqual(correlate([debug], [{ archive: 'other.lib', member: 'lib.c.obj' }]), []);
});

test('HS2-9E879K confirms a current link only when it both warns about MSVCRTD and loads an MSVCRTD requester', (t) => {
  const { code, calls, report, logs } = fixture(t, {
    previous: { ticket: 'HS2-9E879K', status: 'completed', currentLinkConfirmed: true, stale: true },
  });
  assert.equal(code, 0);
  assert.equal(calls.length, 1);
  assert.equal(report.stale, undefined, 'a previous report is replaced, never merged');
  assert.equal(report.status, 'completed');
  assert.equal(report.currentLinkConfirmed, true);
  assert.deepEqual(report.provenance, { unavailable: [], sourceSha: SHA, runId: '9', runAttempt: '2' });
  assert.equal(report.directiveSourceMatches, true);
  assert.equal(report.directiveProvenanceMatches, true);
  assert.deepEqual(
    report.candidateMatches.kept.map((match) => [match.member, match.match, match.loadedArchive]),
    [
      ['lib.c.obj', 'exact', 'vcpkg/installed/x64-windows-static-md/debug/lib/dav1d.lib'],
      ['picture.c.obj', 'exact', 'vcpkg/installed/x64-windows-static-md/debug/lib/dav1d.lib'],
    ],
  );
  assert.equal(report.candidates, 3);
  assert.equal(report.loadedMembers, 2);
  assert.deepEqual(report.lnk4098, { MSVCRTD: 1 });
  // Identifier-only evidence: no machine-local roots or raw linker log in the artifact.
  const text = JSON.stringify(report);
  assert.ok(!text.includes('C:\\\\vcpkg') && !text.includes('Referenced in') && !text.includes('Found dav1d'));
  assert.ok(logs.some((line) => line.includes('currentLinkConfirmed=true')));
});

test('HS2-9E879K does not confirm without an LNK4098 emission or with only ambiguous matches', (t) => {
  const quiet = fixture(t, {
    stdout: cargoStream(
      VERBOSE.split('\r\n')
        .filter((line) => !line.includes('LNK4098'))
        .join('\n'),
    ),
  });
  assert.equal(quiet.report.currentLinkConfirmed, false);
  assert.equal(quiet.report.candidateMatches.total, 2);
  const twin = directiveReport();
  twin.requesters.MSVCRTD.push({
    ...twin.requesters.MSVCRTD[0],
    artifact: 'elsewhere/dav1d.lib',
    kind: 'vcpkg-release',
  });
  const bareOnly = [
    'Searching libraries',
    '      Loaded dav1d.lib(lib.c.obj)',
    "LINK : warning LNK4098: defaultlib 'MSVCRTD' conflicts with use of other libs",
  ].join('\n');
  const ambiguous = fixture(t, { directives: twin, stdout: cargoStream(bareOnly) });
  assert.ok(ambiguous.report.candidateMatches.kept.every((match) => match.match === 'ambiguous'));
  assert.equal(ambiguous.report.currentLinkConfirmed, false);
});

test('HS2-9E879K replaces earlier evidence with an explicit failure for every unusable input', (t) => {
  const previous = { ticket: 'HS2-9E879K', status: 'completed', currentLinkConfirmed: true };
  const cases = [
    [{ directives: null }, 2, 'directive_report_missing'],
    [{ directives: '{ not json' }, 2, 'directive_report_unreadable'],
    [{ directives: 'null' }, 2, 'directive_report_incomplete'],
    [{ directives: directiveReport({ status: 'failed' }) }, 2, 'directive_report_incomplete'],
    [{ error: true }, 3, 'cargo_unavailable'],
    [{ stdout: cargoStream('nothing verbose here') }, 4, 'no_verbose_linker_output'],
  ];
  for (const [options, expected, failure] of cases) {
    const { code, report, calls } = fixture(t, { ...options, previous });
    assert.equal(code, expected, failure);
    assert.equal(report.status, 'failed', failure);
    assert.equal(report.failure, failure);
    assert.equal(report.currentLinkConfirmed, false, failure);
    if (failure.startsWith('directive_')) assert.equal(calls.length, 0, 'no relink without directive evidence');
  }
  const failedLink = fixture(t, { status: 101 });
  assert.equal(failedLink.code, 0);
  assert.equal(failedLink.report.status, 'completed_link_failed');
  assert.equal(failedLink.report.cargoExit, 101);
});

test('HS2-9E879K bounds every kept list while recording the true totals', (t) => {
  const many = Array.from({ length: LIMITS.searched + 25 }, (_, index) => `    Searching C:\\libs\\lib${index}.lib:`);
  const { report } = fixture(t, { stdout: cargoStream([...many, VERBOSE].join('\n')) });
  assert.equal(report.librariesSearched.kept.length, LIMITS.searched);
  assert.equal(report.librariesSearched.total, LIMITS.searched + 25 + 3);
  assert.equal(report.provenance.unavailable.length, 0);
});

test('HS2-9E879K refuses stale, missing and malformed provenance before relinking and replaces old positives', (t) => {
  const valid = directiveReport().provenance;
  for (const field of ['sourceSha', 'runId', 'runAttempt']) {
    for (const value of [undefined, null, '', 'invalid', field === 'sourceSha' ? 'b'.repeat(40) : '123']) {
      const result = fixture(t, {
        directives: directiveReport({ provenance: { ...valid, [field]: value } }),
        previous: { currentLinkConfirmed: true, oldEvidence: true },
      });
      assert.equal(result.code, 2, `${field}=${value}`);
      assert.equal(result.report.failure, 'directive_provenance_mismatch');
      assert.equal(result.report.currentLinkConfirmed, false);
      assert.equal(result.report.oldEvidence, undefined);
      assert.equal(result.calls.length, 0);
    }
  }
  for (const provenance of [undefined, null, {}, { ...valid, unavailable: ['sourceSha'] }]) {
    const result = fixture(t, { directives: directiveReport({ provenance }) });
    assert.equal(result.code, 2);
    assert.equal(result.report.currentLinkConfirmed, false);
    assert.equal(result.calls.length, 0);
  }
  for (const variable of ['GITHUB_SHA', 'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT']) {
    for (const value of [undefined, 'invalid']) {
      const result = fixture(t, { env: { [variable]: value } });
      assert.equal(result.code, 2);
      assert.equal(result.report.currentLinkConfirmed, false);
      assert.equal(result.calls.length, 0);
    }
  }
  const sameShaCase = fixture(t, {
    directives: directiveReport({ provenance: { ...valid, sourceSha: SHA.toUpperCase() } }),
  });
  assert.equal(sameShaCase.report.currentLinkConfirmed, true);
  assert.equal(sameShaCase.report.directiveSourceMatches, true);
});

test('HS2-9E879K retains every CRT directive for an exact loaded member', (t) => {
  const directives = directiveReport();
  directives.requesters.MSVCRT = [{ ...directives.requesters.MSVCRTD[0] }];
  const { report } = fixture(t, { directives });
  assert.equal(report.currentLinkConfirmed, true);
  assert.deepEqual(
    report.candidateMatches.kept.filter((match) => match.member === 'lib.c.obj').map((match) => match.library),
    ['MSVCRTD', 'MSVCRT'],
  );
});

test('HS2-9E879K replaces a prior positive when Cargo cannot resolve a unique requested target', (t) => {
  for (const metadata of [
    { error: { code: 'ENOENT' } },
    { status: 101, stdout: '' },
    { status: 0, stdout: '{' },
    { status: 0, stdout: '{}' },
    { status: 0, stdout: JSON.stringify({ packages: [{ name: 'other', id: PACKAGE_ID, targets: [TARGET] }] }) },
    {
      status: 0,
      stdout: JSON.stringify({ packages: [{ name: 'hotsheet-ticketing', id: PACKAGE_ID, targets: [TARGET, TARGET] }] }),
    },
  ]) {
    const result = fixture(t, { metadata, previous: { currentLinkConfirmed: true, oldEvidence: true } });
    assert.equal(result.code, 3);
    assert.equal(result.report.currentLinkConfirmed, false);
    assert.equal(result.report.oldEvidence, undefined);
    assert.equal(result.calls.length, 0);
  }
});

test('HS2-9E879K never calls a privacy-reduced legacy member identity exact', (t) => {
  const legacy = directiveReport();
  legacy.requesters.MSVCRTD = [
    { artifact: 'vcpkg/installed/x64-windows-static-md/debug/lib/dav1d.lib', member: 'thread.obj', kind: 'synthetic' },
  ];
  const result = fixture(t, {
    directives: legacy,
    stdout: cargoStream(
      `Loaded ${DEBUG_DAV1D}(C:\\build\\other\\thread.obj)\nLINK : warning LNK4098: defaultlib 'MSVCRTD' conflicts`,
    ),
  });
  assert.equal(result.report.currentLinkConfirmed, false);
  assert.equal(result.report.candidateMatches.kept[0].match, 'basename');
});

test('HS2-9E879K distinguishes same-basename members and rejects malformed member identities', (t) => {
  const raw = 'C:\\build\\other\\thread.obj',
    stdout = cargoStream(`Loaded ${DEBUG_DAV1D}(${raw})\nLINK : warning LNK4098: defaultlib 'MSVCRTD' conflicts`);
  for (const identity of [
    undefined,
    null,
    {},
    { version: 2, sha256: 'a'.repeat(64) },
    { version: 1, sha256: 'invalid' },
    memberIdentity('C:/build/first/thread.obj'),
    memberIdentity('thread.obj'),
    memberIdentity(raw),
  ]) {
    const directives = directiveReport();
    directives.requesters.MSVCRTD = [
      {
        artifact: 'vcpkg/installed/x64-windows-static-md/debug/lib/dav1d.lib',
        member: 'thread.obj',
        memberIdentity: identity,
        kind: 'synthetic',
      },
    ];
    const result = fixture(t, { directives, stdout });
    assert.equal(result.report.currentLinkConfirmed, identity?.sha256 === memberIdentity(raw).sha256);
    assert.equal(
      result.report.candidateMatches.kept[0].match,
      result.report.currentLinkConfirmed ? 'exact' : 'basename',
    );
    assert.ok(!JSON.stringify(result.report).includes('C:\\\\build'), 'no raw member root in report');
  }
});

test('HS2-9E879K retains the exact matched digest for same-archive same-basename report audit', (t) => {
  const first = memberIdentity('C:/build/a/thread.obj'),
    second = memberIdentity('C:/build/b/thread.obj'),
    directives = directiveReport();
  directives.requesters.MSVCRTD = [first, second].map((identity) => ({
    artifact: 'vcpkg/installed/x64-windows-static-md/debug/lib/dav1d.lib',
    member: 'thread.obj',
    memberIdentity: identity,
    kind: 'synthetic',
  }));
  const result = fixture(t, {
    directives,
    stdout: cargoStream(
      `Loaded ${DEBUG_DAV1D}(C:/build/a/thread.obj)\nLINK : warning LNK4098: defaultlib 'MSVCRTD' conflicts`,
    ),
  });
  assert.equal(result.report.currentLinkConfirmed, true);
  assert.equal(result.report.currentLinkMatches.total, 1);
  assert.deepEqual(result.report.currentLinkMatches.kept[0].memberIdentity, first);
  assert.deepEqual(result.report.candidateMatches.kept[0].memberIdentity, first);
  assert.notDeepEqual(result.report.currentLinkMatches.kept[0].memberIdentity, second);
  assert.ok(!JSON.stringify(result.report).includes('build/a'), 'digest remains private');
});

test('HS2-9E879K confirms only a fresh exact target diagnostic without borrowing unrelated evidence', (t) => {
  const other = { package_id: 'other', target: { ...TARGET, name: 'dependency' } };
  const unrelatedWarning = JSON.stringify({
    ...JSON.parse(event("linker stdout: LINK : warning LNK4098: defaultlib 'MSVCRTD' conflicts")),
    ...other,
  });
  const selected = event(`linker stdout: Loaded ${DEBUG_DAV1D}(C:\\build\\dav1d\\src\\lib.c.obj)`);
  assert.equal(
    fixture(t, { stdout: [artifact(), selected, unrelatedWarning].join('\n') }).report.currentLinkConfirmed,
    false,
  );
  const positive = fixture(t, {
    stdout: [artifact(), unrelatedWarning, event(`linker stdout: ${VERBOSE}`)].join('\n'),
  });
  assert.equal(positive.report.currentLinkConfirmed, true);
  assert.deepEqual(
    positive.report.currentLinkMatches.kept.map((match) => match.member),
    ['lib.c.obj', 'picture.c.obj'],
  );
  assert.deepEqual(positive.report.currentTarget, {
    freshArtifact: true,
    artifacts: 1,
    linkDiagnostics: 1,
    confirmingDiagnostics: 1,
  });
  const repeated = fixture(t, {
    stdout: [artifact(), event(`linker stdout: ${VERBOSE}`), event(`linker stdout: ${VERBOSE}`)].join('\n'),
  });
  assert.equal(
    repeated.report.currentLinkConfirmed,
    false,
    'multiple possible link diagnostics are not one proven current link',
  );
});

test('HS2-9E879K never confirms basename-only, different archive or outside-root sanitization matches', (t) => {
  const warning = "LINK : warning LNK4098: defaultlib 'MSVCRTD' conflicts with use of other libs";
  for (const archive of [RELEASE_DAV1D, 'dav1d.lib', 'C:\\elsewhere\\dav1d.lib']) {
    const result = fixture(t, { stdout: cargoStream(`Loaded ${archive}(lib.c.obj)\n${warning}`) });
    assert.equal(result.code, 0);
    assert.equal(result.report.currentLinkConfirmed, false, archive);
    assert.equal(result.report.candidateMatches.kept[0].match, 'basename');
  }
  // Both sanitized names are equal, but neither archive's full identity is known.
  const directives = directiveReport();
  directives.requesters.MSVCRTD = [{ artifact: 'dav1d.lib', member: 'lib.c.obj', kind: 'synthetic' }];
  const result = fixture(t, {
    directives,
    stdout: cargoStream(`Loaded C:\\elsewhere\\dav1d.lib(lib.c.obj)\n${warning}`),
  });
  assert.equal(result.report.currentLinkConfirmed, false);
  assert.equal(result.report.candidateMatches.kept[0].match, 'basename');
  assert.ok(!JSON.stringify(result.report).includes('elsewhere'));
});

test('HS2-9E879K normalizes archive separators, case and dot segments for qualified exact matches', () => {
  const candidate = {
    library: 'MSVCRTD',
    artifact: 'target/debug/lib/dav1d.lib',
    member: 'thread.obj',
    memberIdentity: memberIdentity('thread.obj'),
  };
  for (const archive of ['TARGET\\debug\\lib\\DAV1D.LIB', 'target/debug/obj/../lib/dav1d.lib']) {
    assert.equal(
      correlate([candidate], [{ archive, member: 'THREAD.OBJ', memberIdentity: memberIdentity('THREAD.OBJ') }])[0]
        .match,
      'exact',
    );
  }
  assert.equal(correlate([candidate], [{ archive: 'target/../dav1d.lib', member: 'thread.obj' }])[0].match, 'basename');
});

test('HS2-9E879K registers its real regression suite in both root gates', () => {
  for (const path of ['../scripts/check.sh', '../.github/workflows/ci.yml']) {
    const source = readFileSync(new URL(path, import.meta.url), 'utf8');
    const command = source
      .split('\n')
      .find((line) => line.includes('node --test') && line.includes('scripts/windows-crt-directives.test.mjs'));
    assert.ok(command?.includes('scripts/windows-crt-link-correlation.test.mjs'), path);
    assert.ok(command.includes('scripts/throughput-baseline.test.mjs'), 'preserve existing registrations');
  }
});

test('HS2-9E879K correlates after the directive report in the native job, unmasked and flag-free', () => {
  const workflow = readFileSync(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8');
  const job = workflow.slice(workflow.indexOf('  windows-credential-store:'), workflow.indexOf('\n  check:'));
  const order = [
    'cargo test -p hotsheet-ticketing --test file_lock_process',
    'path: target/crt-directives/report.json',
    'node scripts/windows-crt-link-correlation.mjs --target target --directives target/crt-directives/report.json --output target/crt-directives/link-correlation.json',
    'path: target/crt-directives/link-correlation.json',
  ].map((step) => job.indexOf(step));
  assert.ok(
    order.every((position) => position > 0),
    'the affected step, both reports and both uploads stay in the job',
  );
  assert.deepEqual(
    [...order].sort((left, right) => left - right),
    order,
    'correlation follows the directive report',
  );
  const steps = job.slice(job.indexOf('Correlate CRT candidates with one current affected link'));
  assert.equal(steps.match(/if: \$\{\{ !cancelled\(\) \}\}/g)?.length, 2);
  assert.match(steps, /CC: cl/);
  assert.match(steps, /name: windows-crt-link-correlation/);
  assert.match(steps, /if-no-files-found: error/);
  assert.doesNotMatch(job, /NODEFAULTLIB|crt-static|\/MDd?\b/i);
});
