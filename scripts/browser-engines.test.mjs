import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import {
  ciBrowserMatrix,
  commandCoverage,
  coverageSummary,
  defaultEngine,
  formatReportSummary,
  gateBrowserCoverage,
  matrixTable,
  playwrightInvocation,
  reportEngines,
  specEngines,
  workflowRuns,
} from './browser-engines.mjs';
import { browserEvidence } from './local-gate.mjs';

const repo = new URL('..', import.meta.url).pathname;
const read = (path) => readFileSync(join(repo, path), 'utf8');

const specs = {
  'tests/plain.spec.ts': { use: null, launched: [], optIn: [] },
  'tests/safari.spec.ts': { use: null, launched: ['webkit'], optIn: [] },
  'tests/chromium-launch.spec.ts': { use: null, launched: ['chromium'], optIn: [] },
  'tests/profile.spec.ts': { use: 'firefox', launched: [], optIn: ['HOTSHEET_PROFILE'] },
};

test('HS2-HW6VED the config default is Chromium and an unknown config shape is refused', () => {
  assert.equal(defaultEngine(read('clients/web/playwright.config.ts')), 'chromium');
  assert.equal(defaultEngine("export default defineConfig({ use: { browserName: 'webkit' } });"), 'webkit');
  assert.throws(() => defaultEngine('export default defineConfig({ projects: [] });'), /projects/);
  assert.throws(() => defaultEngine("use: { browserName: 'netscape' }"), /Unknown/);
});

test('HS2-HW6VED specs report launched engines, browserName overrides and whole-file opt-ins', () => {
  assert.deepEqual(
    specEngines(
      "import { expect, test, webkit, type Page } from '@playwright/test';\nconst b = await webkit.launch();",
    ),
    { use: null, launched: ['webkit'], optIn: [] },
  );
  // Importing an engine without launching it, or only as a type, is not a launch.
  assert.deepEqual(specEngines("import { chromium, test } from '@playwright/test';\n").launched, []);
  assert.deepEqual(specEngines("import { type webkit, test } from '@playwright/test';\nx.launch();").launched, []);
  // A file-level opt-in skips the whole spec.
  assert.deepEqual(
    specEngines(
      "import { test } from '@playwright/test';\ntest.use({ browserName: 'firefox' });\ntest.skip(process.env.HOTSHEET_X !== '1', 'opt in');\ntest('a', () => {});",
    ),
    { use: 'firefox', launched: [], optIn: ['HOTSHEET_X'] },
  );
  // Every declared test gated: opt-in. One gated test among several: not opt-in.
  const gated = "  test.skip(\n    process.env.HOTSHEET_Y !== '1',\n    'why',\n  );";
  assert.deepEqual(specEngines(`test('a', () => {\n${gated}\n});`).optIn, ['HOTSHEET_Y']);
  assert.deepEqual(specEngines(`test('a', () => {\n${gated}\n});\ntest('b', () => {});`).optIn, []);
  // A skip without an environment flag is not an opt-in.
  assert.deepEqual(specEngines("test.skip(browserName !== 'chromium', 'x');\ntest('a', () => {});").optIn, []);
});

test('HS2-HW6VED a command resolves npm scripts and ignores non-test Playwright commands', () => {
  const scripts = { 'test:e2e': 'npm run test:fixtures && playwright test', 'test:fixtures': 'node build.mjs' };
  assert.equal(playwrightInvocation('npm run test:e2e', scripts), 'playwright test');
  assert.equal(playwrightInvocation('npm run test:fixtures', scripts), null);
  assert.equal(playwrightInvocation('npm run missing', scripts), null);
  assert.equal(playwrightInvocation('npx playwright merge-reports --reporter=html ./blobs'), null);
  assert.equal(
    playwrightInvocation('dbus-run-session -- bash wrap.sh npx playwright test --shard=1/4 && echo done'),
    'playwright test --shard=1/4',
  );
});

test('HS2-HW6VED a focused WebKit command is never reported as a full engine matrix', () => {
  const focused = commandCoverage("playwright test tests/plain.spec.ts --browser=webkit --grep '@ci-webkit'", {
    engine: 'chromium',
    specs,
  });
  assert.deepEqual(focused, {
    defaultEngine: 'webkit',
    engines: ['webkit'],
    files: ['tests/plain.spec.ts'],
    grep: ['@ci-webkit'],
    specLaunched: {},
    optIn: [],
  });
  const full = commandCoverage('npx playwright test --shard=2/4 --fully-parallel', { engine: 'chromium', specs });
  assert.equal(full.defaultEngine, 'chromium');
  assert.equal(full.files, null, 'no files means the whole suite');
  assert.deepEqual(full.engines, ['chromium', 'webkit']);
  // A spec launching the default engine adds nothing; an opt-in spec is reported, not counted.
  assert.deepEqual(full.specLaunched, { webkit: ['tests/safari.spec.ts'] });
  assert.deepEqual(full.optIn, ['tests/profile.spec.ts']);
  assert.equal(coverageSummary([full]), 'chromium: all specs / webkit: spec-launched in safari');
  assert.equal(
    coverageSummary([focused]),
    'webkit: plain; grep @ci-webkit',
    'the summary names the focused selection, not the suite',
  );
  assert.equal(coverageSummary([]), 'no Playwright tests');
  assert.deepEqual(
    commandCoverage('npx playwright test tests/plain.spec.ts --browser all', { engine: 'chromium', specs }).engines,
    ['chromium', 'firefox', 'webkit'],
  );
  assert.equal(commandCoverage('npx playwright test -g x', { engine: 'chromium', specs }).grep[0], 'x');
  assert.throws(
    () => commandCoverage('npx playwright test --project=webkit', { engine: 'chromium', specs }),
    /projects/,
  );
  assert.throws(() => commandCoverage('npx playwright test --browser=lynx', { engine: 'chromium', specs }), /Unknown/);
});

test('HS2-HW6VED workflow steps are read per job, including block scalars and continuations', () => {
  const runs = workflowRuns(
    [
      'on:',
      '  push:',
      '    branches: [main]',
      'jobs:',
      '  first:',
      '    steps:',
      '      - name: One',
      '        run: npx playwright test',
      '      - name: Block',
      '        run: |',
      '          echo start',
      '          npx playwright test \\',
      '            --browser=webkit',
      '',
      '  second:',
      '    steps:',
      '      - uses: actions/checkout@v4',
      '      - run: npm run test:e2e',
      '      - name: Tail',
      '        run: >-',
      '          npx playwright test tests/a.spec.ts',
    ].join('\n'),
  );
  assert.deepEqual(runs, [
    { job: 'first', step: 'One', run: 'npx playwright test' },
    { job: 'first', step: 'Block', run: 'echo start' },
    { job: 'first', step: 'Block', run: 'npx playwright test --browser=webkit' },
    { job: 'second', step: null, run: 'npm run test:e2e' },
    { job: 'second', step: 'Tail', run: 'npx playwright test tests/a.spec.ts' },
  ]);
});

test('HS2-HW6VED the documented browser engine matrix equals the one derived from the workflows', () => {
  const doc = read('docs/12-code-organization-and-testing.md');
  const match = /<!-- browser-engine-matrix:begin -->([\s\S]*?)<!-- browser-engine-matrix:end -->/.exec(doc);
  assert.ok(match, 'docs/12 keeps the browser engine matrix between its markers');
  // Prettier pads table cells; compare cells, not spacing.
  const cells = (text) =>
    text
      .trim()
      .split('\n')
      .filter((line) => line.startsWith('|') && !/^\|[\s|:-]+\|$/.test(line))
      .map((line) =>
        line
          .slice(1, -1)
          .split('|')
          .map((cell) => cell.trim()),
      );
  const rows = ciBrowserMatrix(repo);
  assert.deepEqual(
    cells(match[1]),
    cells(matrixTable(rows)),
    'update docs/12 with node scripts/browser-engines.mjs --markdown',
  );
  // The facts the ticket corrected: the full suites are Chromium, and WebKit is the default
  // engine only for the focused @ci-webkit group.
  for (const row of rows.filter((entry) => entry.files === null)) assert.equal(row.defaultEngine, 'chromium');
  const webkit = rows.filter((entry) => entry.defaultEngine === 'webkit');
  assert.deepEqual(
    webkit.map((entry) => [entry.job, entry.grep]),
    [['webkit-popup', ['@ci-webkit']]],
  );
});

test('HS2-HW6VED every full-suite job records its actual engines in the job summary', () => {
  for (const [workflow, job] of [
    ['.github/workflows/ci.yml', 'browser-full-report'],
    ['.github/workflows/platforms.yml', 'core'],
  ]) {
    const runs = workflowRuns(read(workflow)).filter((entry) => entry.job === job);
    assert.ok(
      runs.some(
        (entry) =>
          entry.step === 'Browser engines in this run' &&
          entry.run.includes('scripts/browser-engines.mjs --report playwright-report.json --summary'),
      ),
      `${workflow} ${job} reports the engines of the merged Playwright report`,
    );
  }
});

test('HS2-HW6VED a run report counts tests by default engine and lists spec-launched engines', () => {
  const report = {
    config: { projects: [{ name: '', use: {} }] },
    suites: [
      {
        file: 'plain.spec.ts',
        specs: [{ file: 'plain.spec.ts', tests: [{ status: 'expected' }, { status: 'skipped' }] }],
      },
      {
        file: 'safari.spec.ts',
        specs: [],
        suites: [{ specs: [{ file: 'safari.spec.ts', tests: [{ status: 'flaky' }, { status: 'skipped' }] }] }],
      },
      { file: 'profile.spec.ts', specs: [{ file: 'profile.spec.ts', tests: [{ status: 'skipped' }] }] },
    ],
  };
  const result = reportEngines(report, { engine: 'chromium', specs });
  assert.deepEqual(result, {
    byEngine: { chromium: { ran: 2, skipped: 2 }, firefox: { ran: 0, skipped: 1 } },
    launched: { webkit: { 'tests/safari.spec.ts': { ran: 1 } } },
  });
  const summary = formatReportSummary(result);
  assert.match(summary, /\| chromium \| 2 \| 2 \| default engine \|/);
  assert.match(summary, /\| webkit \| 1 in launching files \| - \| spec-launched in safari \|/);
  assert.match(summary, /Only the default engine runs the whole selection/);
  assert.throws(
    () => reportEngines({ config: { projects: [{ use: { browserName: 'webkit' } }] } }, { engine: 'chromium', specs }),
    /project browsers/,
  );
});

test('HS2-HW6VED gate evidence annotates selected and deferred browser stages with real engines', (t) => {
  const evidence = browserEvidence(
    repo,
    [
      { cwd: '.', command: 'cargo lint' },
      { cwd: 'clients/web', command: 'npm run test:fixtures' },
      { cwd: 'clients/web', command: "npx playwright test 'tests/terminal-safari.spec.ts'" },
    ],
    [{ cwd: 'clients/web', command: 'npm run test:ci:webkit' }],
  );
  assert.equal(evidence.local.stages.length, 1, 'only Playwright test stages are annotated');
  assert.deepEqual(evidence.local.stages[0].engines, ['chromium', 'webkit']);
  assert.equal(evidence.local.summary, 'chromium: terminal-safari / webkit: spec-launched in terminal-safari');
  assert.equal(evidence.deferred.stages[0].defaultEngine, 'webkit');
  assert.match(evidence.deferred.summary, /^webkit: native-prompt-replacements, providers; grep @ci-webkit$/);
  assert.ok(evidence.ci.some((row) => row.job === 'browser-full' && row.files === null));
  assert.deepEqual(gateBrowserCoverage([], repo), { stages: [], summary: 'no Playwright tests' });
  const empty = mkdtempSync(join(tmpdir(), 'hs2-hw6ved-'));
  t.after(() => rmSync(empty, { recursive: true, force: true }));
  assert.equal(browserEvidence(empty, [], []), null, 'a repository without the web client reports no coverage');
});
