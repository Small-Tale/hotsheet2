// HS2-HAHX20: each CI failure format parses to identifiers only. All log text is synthetic.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  combineAttempts,
  EXCERPT_LIMIT,
  identity,
  parseJUnit,
  parseLog,
  parsePlaywrightJson,
  parseReport,
  parseVitestJson,
  splitLog,
} from './ci-failure-parse.mjs';

const fixtures = join(dirname(fileURLToPath(import.meta.url)), 'fixtures/ci-failure-attribution');
const gh = (job, step, lines) => lines.map((text, index) => `${job}\t${step}\t2026-11-05T06:00:0${index % 10}.1234567Z ${text}`).join('\n');
const ids = (entries) => entries.map(identity).sort();

test('HS2-HAHX20 splits gh --log-failed lines into job, step and text and strips ANSI and timestamps', () => {
  const [line, bare] = splitLog('job a\tstep b\t﻿2026-11-05T06:00:00.0000000Z \u001b[31mFAIL\u001b[0m x\nplain text');
  assert.deepEqual(line, { job: 'job a', step: 'step b', text: 'FAIL x' });
  assert.deepEqual(bare, { job: '', step: '', text: 'plain text' });
});

test('HS2-HAHX20 nextest: final failures, timeouts and signals, deduplicated; retried passes are flaky', () => {
  const parsed = parseLog(gh('rust', 'Test', [
    '        FAIL [   0.120s] (  3/ 40) demo-core parser::tests::parses_header',
    '     TIMEOUT [  60.000s] demo-cli::cli waits_forever',
    '     SIGSEGV [   0.010s] demo-core::bin/demo native::crashes',
    '  TRY 1 FAIL [   0.300s] demo-cli::cli retries_lock',
    '  TRY 2 PASS [   0.200s] demo-cli::cli retries_lock',
    '   FLAKY 2/3 [   0.200s] demo-cli::cli other_retry',
    '        FAIL [   0.120s] demo-core parser::tests::parses_header',
    '##[error]Process completed with exit code 100.',
  ]));
  assert.deepEqual(ids(parsed.failures), [
    'nextest:demo-cli::cli waits_forever',
    'nextest:demo-core parser::tests::parses_header',
    'nextest:demo-core::bin/demo native::crashes',
  ]);
  assert.deepEqual(ids(parsed.flaky), ['nextest:demo-cli::cli other_retry', 'nextest:demo-cli::cli retries_lock']);
  const header = parsed.failures.find((failure) => failure.test.endsWith('parses_header'));
  assert.equal(header.binary, 'demo-core');
  assert.equal(header.job, 'rust');
  assert.equal(header.step, 'Test');
});

test('HS2-HAHX20 vitest: test and file-level failures, with and without a project label', () => {
  const parsed = parseLog(gh('web', 'unit', [
    ' FAIL  src/widget.test.ts > widget > renders the label',
    ' FAIL  |unit| src/a/b.test.tsx > nested > case',
    ' FAIL  src/broken.test.ts [ src/broken.test.ts ]',
    ' FAIL  src/widget.test.ts > widget > renders the label',
    '##[error]Process completed with exit code 1.',
  ]));
  assert.deepEqual(ids(parsed.failures), [
    'vitest:src/a/b.test.tsx > nested > case',
    'vitest:src/broken.test.ts',
    'vitest:src/widget.test.ts > widget > renders the label',
  ]);
  assert.equal(parsed.failures.find((failure) => failure.test === 'src/broken.test.ts').file, 'src/broken.test.ts');
});

test('HS2-HAHX20 Playwright: the summary sections decide failed and flaky; one failure covers every browser', () => {
  const parsed = parseLog(gh('browser (shard 1/4)', 'Run', [
    '  1) [chromium] › tests/board.spec.ts:10:3 › board › opens a card ────────────',
    '  2) [webkit] › tests/board.spec.ts:10:3 › board › opens a card (retry #1) ────',
    '  3) [chromium] › tests/menu.spec.ts:4:1 › menu closes ───',
    '  1 failed',
    '    [chromium] › tests/board.spec.ts:10:3 › board › opens a card ',
    '  1 flaky',
    '    [chromium] › tests/menu.spec.ts:4:1 › menu closes ',
    '  9 passed (20s)',
  ]));
  assert.deepEqual(ids(parsed.failures), ['playwright:tests/board.spec.ts › board › opens a card']);
  assert.deepEqual(parsed.failures[0].projects.sort(), ['chromium', 'webkit']);
  assert.equal(parsed.failures[0].excerpt, '1) [chromium] › tests/board.spec.ts:10:3 › board › opens a card');
  assert.deepEqual(ids(parsed.flaky), ['playwright:tests/menu.spec.ts › menu closes']);
});

test('HS2-HAHX20 Playwright without a summary: every numbered header is a failure', () => {
  const parsed = parseLog(gh('browser', 'Run', ['  1) [chromium] › tests/a.spec.ts:1:1 › one ──', '##[error]Process completed with exit code 1.']));
  assert.deepEqual(ids(parsed.failures), ['playwright:tests/a.spec.ts › one']);
  assert.deepEqual(parsed.flaky, []);
});

test('HS2-HAHX20 node --test: spec failing-test summary and TAP with locations, ignoring suites', () => {
  const spec = parseLog(gh('root', 'node tests', ['✖ failing tests:', '', 'test at scripts/tool.test.mjs:5:1', '✖ tool formats output (1.5ms)', '  AssertionError']));
  assert.deepEqual(ids(spec.failures), ['node-test:scripts/tool.test.mjs > tool formats output']);
  assert.equal(spec.failures[0].file, 'scripts/tool.test.mjs');
  const tap = parseLog([
    'TAP version 13',
    '# Subtest: suite',
    '    not ok 1 - inner case',
    '      ---',
    "      location: '/home/runner/work/repo/repo/scripts/tool.test.mjs:9:3'",
    '      ...',
    'not ok 1 - suite',
    '  ---',
    "  location: '/home/runner/work/repo/repo/scripts/tool.test.mjs:8:1'",
    '  ...',
    'not ok 2 - second top-level',
    '  ---',
    "  location: 'file:///home/runner/work/repo/repo/scripts/tool.test.mjs:20:1'",
    '  ...',
  ].join('\n'));
  assert.deepEqual(ids(tap.failures), [
    'node-test:/home/runner/work/repo/repo/scripts/tool.test.mjs > inner case',
    'node-test:/home/runner/work/repo/repo/scripts/tool.test.mjs > second top-level',
  ]);
});

test('HS2-HAHX20 step diagnostics: rustc errors (not warnings), rustfmt, tsc, ESLint and Prettier locations', () => {
  const parsed = parseLog([
    gh('rust', 'Clippy', ['error: unused variable: `x`', '  --> crates/demo-core/src/lexer.rs:4:9', 'warning: dead code', '  --> crates/demo-core/src/other.rs:1:1', 'error[E0308]: mismatched types', '  --> crates/demo-core/src/parser.rs:2:2']),
    gh('rust', 'Format', ['Diff in /home/runner/work/repo/repo/crates/demo-core/src/lib.rs at line 3:']),
    gh('web', 'typecheck', ['src/widget.ts(12,3): error TS2322: Type mismatch.', 'src/label.ts:4:1 - error TS2304: Cannot find name.']),
    gh('web', 'lint', ['/home/runner/work/repo/repo/clients/web/src/widget.ts', '  12:5  error  Unexpected any  @typescript-eslint/no-explicit-any', '']),
    gh('web', 'format', ['Checking formatting...', '[warn] src/label.ts', '[warn] Code style issues found in the above file. Run Prettier with --write to fix.']),
  ].join('\n'));
  const byStep = Object.fromEntries(parsed.failures.map((failure) => [`${failure.framework}:${failure.step}`, failure.locations.sort()]));
  assert.deepEqual(byStep, {
    'rustc:Clippy': ['crates/demo-core/src/lexer.rs', 'crates/demo-core/src/parser.rs'],
    'rustfmt:Format': ['/home/runner/work/repo/repo/crates/demo-core/src/lib.rs'],
    'tsc:typecheck': ['src/label.ts', 'src/widget.ts'],
    'eslint:lint': ['/home/runner/work/repo/repo/clients/web/src/widget.ts'],
    'prettier:format': ['src/label.ts'],
  });
  assert.ok(parsed.failures.every((failure) => failure.kind === 'diagnostic'));
  assert.equal(identity(parsed.failures[0]), 'rustc:rust / Clippy');
});

test('HS2-HAHX20 a failed step with no recognizable output is one step failure; a clean step is none', () => {
  const parsed = parseLog([
    gh('migrator', 'Run', ['Segmentation fault (core dumped)', '##[error]Process completed with exit code 139.']),
    gh('migrator', 'Setup', ['all good']),
  ].join('\n'));
  assert.deepEqual(ids(parsed.failures), ['step:migrator / Run']);
  assert.equal(parsed.failures[0].excerpt, '##[error]Process completed with exit code 139.');
});

test('HS2-HAHX20 excerpts are single identifier lines bounded in length', () => {
  const parsed = parseLog(gh('web', 'unit', [` FAIL  src/a.test.ts > ${'x'.repeat(400)}`]));
  assert.ok(parsed.failures[0].excerpt.length <= EXCERPT_LIMIT);
});

test('HS2-HAHX20 JUnit, vitest JSON and Playwright JSON reports', () => {
  const junit = parseJUnit(`<testsuites><testsuite name="demo-core">
    <testcase name="parser::tests::parses_header" classname="demo-core"><failure message="x"/></testcase>
    <testcase name="lexer::tests::ok" classname="demo-core"/>
    <testcase name="retries_lock" classname="demo-cli::cli"><flakyFailure message="y"/></testcase>
  </testsuite></testsuites>`);
  assert.deepEqual(ids(junit.failures), ['nextest:demo-core parser::tests::parses_header']);
  assert.deepEqual(ids(junit.flaky), ['nextest:demo-cli::cli retries_lock']);

  const vitest = parseVitestJson({
    testResults: [
      { name: '/repo/clients/web/src/widget.test.ts', status: 'failed', assertionResults: [{ status: 'failed', fullName: 'widget renders', title: 'renders' }, { status: 'passed', fullName: 'ok' }] },
      { name: '/repo/clients/web/src/broken.test.ts', status: 'failed', assertionResults: [] },
    ],
  }, { root: '/repo/clients/web' });
  assert.deepEqual(ids(vitest.failures), ['vitest:src/broken.test.ts', 'vitest:src/widget.test.ts > widget renders']);

  const playwright = parsePlaywrightJson({
    suites: [{ title: 'board.spec.ts', file: 'board.spec.ts', specs: [], suites: [{ title: 'board', specs: [
      { title: 'opens a card', file: 'board.spec.ts', tests: [{ status: 'unexpected', projectName: 'chromium' }, { status: 'unexpected', projectName: 'webkit' }] },
      { title: 'closes', file: 'board.spec.ts', tests: [{ status: 'flaky', projectName: 'chromium' }] },
      { title: 'passes', file: 'board.spec.ts', tests: [{ status: 'expected', projectName: 'chromium' }] },
    ] }] }],
  });
  assert.deepEqual(ids(playwright.failures), ['playwright:board.spec.ts › board › opens a card']);
  assert.deepEqual(playwright.failures[0].projects, ['chromium', 'webkit']);
  assert.deepEqual(ids(playwright.flaky), ['playwright:board.spec.ts › board › closes']);

  assert.equal(parseReport('<testsuites/>').failures.length, 0);
  assert.equal(parseReport('{"testResults":[]}').failures.length, 0);
  assert.equal(parseReport('{"suites":[]}').failures.length, 0);
  assert.throws(() => parseReport('{"other":1}'), /unrecognized test report/);
});

test('HS2-HAHX20 attempts: a failure that cleared on re-run is flaky; one that recurs stays a failure', () => {
  const latest = parseLog(readFileSync(join(fixtures, 'attempt-2.log'), 'utf8'));
  const earlier = parseLog(readFileSync(join(fixtures, 'attempt-1.log'), 'utf8'));
  const combined = combineAttempts(latest, [earlier]);
  assert.ok(ids(combined.failures).includes('nextest:demo-core parser::tests::parses_header'));
  const flaky = Object.fromEntries(combined.flaky.map((entry) => [identity(entry), entry.reason]));
  assert.equal(flaky['vitest:src/other.test.ts > other > settles in time'], 'failed on an earlier attempt and cleared on re-run');
  assert.equal(flaky['nextest:demo-cli::cli retries_lock'], 'passed on a retry within the run');
  assert.equal(flaky['nextest:demo-core parser::tests::parses_header'], undefined);
  // A green latest attempt makes every earlier failure flaky.
  const green = combineAttempts({ failures: [], flaky: [] }, [earlier]);
  assert.deepEqual(green.failures, []);
  assert.equal(green.flaky.length, earlier.failures.length);
});
