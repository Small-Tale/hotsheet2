import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import {
  judgeNarrowedRun,
  narrowedCommand,
  partialFromRun,
  readPartial,
  reportCases,
  tokenize,
  writePartial,
} from './gate-case-rerun.mjs';

export function playwrightReport(cases) {
  const files = new Map();
  for (const { file = 'synthetic.spec.ts', line, title, status, project = 'chromium' } of cases) {
    const suite = files.get(file) ?? { title: file, file, specs: [], suites: [] };
    files.set(file, suite);
    const outcome = { passed: 'expected', failed: 'unexpected', skipped: 'skipped', interrupted: 'unexpected' }[status];
    suite.specs.push({
      title,
      file,
      line,
      tests: [{ projectName: project, status: outcome, results: [{ status, duration: 5 }] }],
    });
  }
  return JSON.stringify({ suites: [...files.values()] });
}

const vitestReport = (root, files) =>
  JSON.stringify({
    testResults: files.map(({ file, status = 'passed', cases }) => ({
      name: join(root, file),
      status,
      assertionResults: cases.map(([fullName, caseStatus]) => ({ fullName, title: fullName, status: caseStatus })),
    })),
  });

const junit = (cases) =>
  `<testsuites>${cases
    .map(
      ([binary, name, failed]) =>
        `<testcase classname="${binary}" name="${name}" time="0.1">${failed ? '<failure message="x"/>' : ''}</testcase>`,
    )
    .join('')}</testsuites>`;

const PLAYWRIGHT = "npx playwright test 'tests/a.spec.ts' 'tests/b.spec.ts' --grep 'smoke|core' --workers 4";

test('HS2-5340N1 tokenizes only plain and single-quoted command words', () => {
  assert.deepEqual(tokenize(PLAYWRIGHT), [
    'npx',
    'playwright',
    'test',
    'tests/a.spec.ts',
    'tests/b.spec.ts',
    '--grep',
    'smoke|core',
    '--workers',
    '4',
  ]);
  for (const unsupported of ['npx playwright test "a.spec.ts"', 'npx playwright test $FILES', 'npx a\\ b', "x 'open"])
    assert.equal(tokenize(unsupported), null, unsupported);
});

test('HS2-5340N1 narrows Playwright to anchored failed locations and keeps its options', () => {
  const failed = [{ selector: 'a.spec.ts:12' }, { selector: 'a.spec.ts:12' }, { selector: 'nested/b.spec.ts:40' }];
  assert.equal(
    narrowedCommand(PLAYWRIGHT, 'playwright', failed),
    "npx playwright test '/a.spec.ts:12' '/nested/b.spec.ts:40' '--grep' 'smoke|core' '--workers' '4'",
  );
  assert.equal(narrowedCommand('npx playwright test "a.spec.ts"', 'playwright', failed), null);
  assert.equal(narrowedCommand('npm run e2e', 'playwright', failed), null);
  assert.equal(narrowedCommand(PLAYWRIGHT, 'playwright', [{ selector: null }]), null);
  assert.equal(narrowedCommand(PLAYWRIGHT, 'playwright', [{ selector: "a'b.spec.ts:1" }]), null);
  assert.equal(narrowedCommand(PLAYWRIGHT, 'playwright', []), null);
});

test('HS2-5340N1 narrows Vitest to failed files and an anchored, escaped full-name pattern', () => {
  const command = narrowedCommand('npm run test:unit', 'vitest', [
    { selector: { file: 'src/a.test.ts', name: 'group keeps (1+1) at $5' } },
    { selector: { file: 'src/b.test.ts', name: "it's ready" } },
  ]);
  assert.equal(
    command,
    "npm run test:unit -- 'src/a.test.ts' 'src/b.test.ts' -t '^(?:group keeps \\(1\\+1\\) at \\$5|it'\\''s ready)$'",
  );
  assert.equal(narrowedCommand('npm run test:other', 'vitest', [{ selector: { file: 'a', name: 'b' } }]), null);
});

test('HS2-5340N1 narrows nextest with exact binary and test filters', () => {
  const base = 'cargo nextest run --all-features --status-level leak';
  assert.equal(
    narrowedCommand(base, 'nextest', [
      { selector: { binary: 'hotsheet-cli::cli', name: 'tests::one' } },
      { selector: { binary: 'hotsheet-core', name: 'two' } },
    ]),
    `${base} -E '(binary_id(=hotsheet-cli::cli) & test(=tests::one)) | (binary_id(=hotsheet-core) & test(=two))'`,
  );
  assert.equal(narrowedCommand(`${base} -E 'test(x)'`, 'nextest', [{ selector: { binary: 'a', name: 'b' } }]), null);
});

test('HS2-5340N1 reads final outcomes and selectors from each runner report', () => {
  const root = '/synthetic/web';
  const playwright = reportCases(
    'playwright',
    playwrightReport([
      { line: 3, title: 'passes', status: 'passed' },
      { line: 9, title: 'fails', status: 'failed' },
      { line: 12, title: 'skips', status: 'skipped' },
    ]),
    root,
  );
  assert.deepEqual(
    playwright.map((entry) => [entry.selector, entry.outcome]),
    [
      ['synthetic.spec.ts:3', 'pass'],
      ['synthetic.spec.ts:9', 'fail'],
      ['synthetic.spec.ts:12', 'skip'],
    ],
  );
  const vitest = reportCases(
    'vitest',
    vitestReport(root, [
      {
        file: 'src/a.test.ts',
        status: 'failed',
        cases: [
          ['suite one', 'failed'],
          ['suite two', 'passed'],
        ],
      },
      { file: 'src/broken.test.ts', status: 'failed', cases: [] },
    ]),
    root,
  );
  assert.deepEqual(
    vitest.map((entry) => [entry.key, entry.outcome, entry.selector]),
    [
      ['src/a.test.ts|suite one', 'fail', { file: 'src/a.test.ts', name: 'suite one' }],
      ['src/a.test.ts|suite two', 'pass', { file: 'src/a.test.ts', name: 'suite two' }],
      ['src/broken.test.ts|', 'fail', null],
    ],
  );
  const nextest = reportCases(
    'nextest',
    junit([
      ['bin', 'ok', false],
      ['bin', 'bad', true],
    ]),
    root,
  );
  assert.deepEqual(
    nextest.map((entry) => [entry.key, entry.outcome]),
    [
      ['bin ok', 'pass'],
      ['bin bad', 'fail'],
    ],
  );
  assert.equal(reportCases('playwright', '{', root), null);
  assert.equal(reportCases('unknown', '{}', root), null);
});

const run = (overrides) => ({
  key: 'stage-key',
  command: PLAYWRIGHT,
  kind: 'playwright',
  status: 1,
  signal: null,
  root: '/synthetic/web',
  body: playwrightReport([
    { line: 3, title: 'passes', status: 'passed' },
    { line: 9, title: 'fails', status: 'failed' },
    { line: 20, title: 'flakes', status: 'failed' },
  ]),
  ...overrides,
});

test('HS2-5340N1 records failed cases only when the whole run is provable', () => {
  const partial = partialFromRun(run());
  assert.equal(partial.total, 3);
  assert.deepEqual(
    partial.failed.map((entry) => entry.selector),
    ['synthetic.spec.ts:9', 'synthetic.spec.ts:20'],
  );
  assert.match(partial.narrowed, /'\/synthetic\.spec\.ts:9' '\/synthetic\.spec\.ts:20'/);
  const interrupted = playwrightReport([
    { line: 3, title: 'fails', status: 'failed' },
    { line: 5, title: 'stopped', status: 'interrupted' },
  ]);
  for (const [reason, value] of [
    ['passing run', { status: 0 }],
    ['signal', { signal: 'SIGINT' }],
    ['missing report', { body: null }],
    ['corrupt report', { body: '{' }],
    ['no failed case', { body: playwrightReport([{ line: 3, title: 'passes', status: 'passed' }]) }],
    ['empty report', { body: JSON.stringify({ suites: [] }) }],
    ['interrupted case', { body: interrupted }],
    ['unsupported command', { command: 'npx playwright test "tests/a.spec.ts"' }],
  ])
    assert.equal(partialFromRun(run(value)), null, reason);
  const brokenFile = vitestReport('/synthetic/web', [{ file: 'src/broken.test.ts', status: 'failed', cases: [] }]);
  assert.equal(
    partialFromRun(run({ kind: 'vitest', command: 'npm run test:unit', body: brokenFile })),
    null,
    'a file-level failure has no rerunnable case',
  );
});

test('HS2-5340N1 judges targeted reruns: pass, narrow again, extra failure, or fall back', () => {
  const partial = partialFromRun(run());
  const report = (nine, twenty, extra = []) =>
    playwrightReport([
      { line: 9, title: 'fails', status: nine },
      { line: 20, title: 'flakes', status: twenty },
      ...extra,
    ]);
  const root = '/synthetic/web';

  const narrowed = judgeNarrowedRun(partial, { status: 1, body: report('failed', 'passed'), root });
  assert.equal(narrowed.result, 'retry');
  assert.deepEqual(
    narrowed.partial.failed.map((entry) => entry.selector),
    ['synthetic.spec.ts:9'],
  );
  assert.deepEqual(narrowed.partial.flaky, ['[chromium] synthetic.spec.ts:20 › flakes']);
  assert.match(narrowed.partial.narrowed, /'\/synthetic\.spec\.ts:9' '--grep'/);
  assert.doesNotMatch(narrowed.partial.narrowed, /:20/);

  const passed = judgeNarrowedRun(narrowed.partial, {
    status: 0,
    body: playwrightReport([{ line: 9, title: 'fails', status: 'passed' }]),
    root,
  });
  assert.deepEqual(passed, {
    result: 'passed',
    flaky: ['[chromium] synthetic.spec.ts:20 › flakes', '[chromium] synthetic.spec.ts:9 › fails'],
  });

  const extra = judgeNarrowedRun(partial, {
    status: 1,
    body: report('passed', 'passed', [{ line: 9, title: 'same line, other test', status: 'failed' }]),
    root,
  });
  assert.equal(extra.result, 'retry');
  assert.deepEqual(
    extra.partial.failed.map((entry) => entry.name),
    ['[chromium] synthetic.spec.ts:9 › same line, other test'],
  );

  for (const [reason, value] of [
    ['signal', { signal: 'SIGTERM', status: 130, body: report('passed', 'passed') }],
    ['missing report', { status: 0, body: null }],
    ['corrupt report', { status: 0, body: '{' }],
    ['case not run', { status: 0, body: playwrightReport([{ line: 9, title: 'fails', status: 'passed' }]) }],
    ['exit without failed case', { status: 1, body: report('passed', 'passed') }],
  ])
    assert.equal(judgeNarrowedRun(partial, { root, ...value }).result, 'full', reason);
});

test('HS2-5340N1 a stored record is reused only for the same stage key and command', (t) => {
  const directory = mkdtempSync(join(tmpdir(), 'hs2-5340n1-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const path = join(directory, 'stage.partial.json');
  const partial = partialFromRun(run());
  writePartial(path, partial);
  assert.deepEqual(readPartial(path, 'stage-key', PLAYWRIGHT), partial);
  assert.equal(readPartial(path, 'other-key', PLAYWRIGHT), null);
  assert.equal(existsSync(path), false, 'a mismatched record is discarded');
  writePartial(path, partial);
  assert.equal(readPartial(path, 'stage-key', 'npx playwright test'), null);
  writeFileSync(path, '{');
  assert.equal(readPartial(path, 'stage-key', PLAYWRIGHT), null);
  assert.equal(existsSync(path), false);
  writeFileSync(path, JSON.stringify({ ...partial, version: 2 }));
  assert.equal(readPartial(path, 'stage-key', PLAYWRIGHT), null);
  assert.equal(readPartial(join(directory, 'absent.json'), 'stage-key', PLAYWRIGHT), null);
});
