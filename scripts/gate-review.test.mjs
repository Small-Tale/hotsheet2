import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

import { buildGateReview, formatGateReview, parseReviewArgs } from './gate-review.mjs';

const day = 86_400_000;
const gate = 'gate:.:0123456789abcdef';
const unreached = 'gate:.:abcdef0123456789';
const file = 'vitest:src/synthetic.test.ts';
const caseId = `${file}#0123456789abcdef`;
const check = (id, outcome, duration, category = null) => ({ id, outcome, ms: duration, category });
const attempt = (id, at, checks, extra = {}) => ({
  id,
  source: 'local',
  cohort: 'local-full-T2',
  os: 'darwin-arm64',
  startedAt: at - 1000,
  endedAt: at,
  selected: [gate, unreached],
  deferred: [],
  checks,
  cases: [],
  ...extra,
});

function fixture() {
  const local = [
    attempt('local:old', day, [check(gate, 'pass', 100), check(file, 'fail', 60, 'test-report-failure')], {
      cases: [check(caseId, 'fail', 20)],
    }),
    attempt('local:current', 9 * day, [check(gate, 'fail', 300, 'exit:1')]),
    attempt('local:reuse', 10 * day, [check(gate, 'reused', 1)], { selected: [gate], deferred: [unreached] }),
  ];
  const ci = [
    {
      id: 'ci:run:1:job:1',
      source: 'ci',
      cohort: 'ci-main',
      os: 'linux-x64',
      runId: 'run',
      runAttempt: '1',
      startedAt: 8 * day,
      endedAt: 8 * day + 200,
      selected: ['ci:test:unit'],
      deferred: [],
      checks: [check('ci:test:unit', 'fail', 200, 'ci:failure')],
      cases: [],
    },
    {
      id: 'ci:run:2:job:1',
      source: 'ci',
      cohort: 'ci-main',
      os: 'linux-x64',
      runId: 'run',
      runAttempt: '2',
      startedAt: 9 * day,
      endedAt: 9 * day + 100,
      selected: ['ci:test:unit'],
      deferred: [],
      checks: [check('ci:test:unit', 'pass', 100)],
      cases: [],
    },
  ];
  return { attempts: [...local, ...ci, ci[0]] }; // CI import replay is idempotent.
}

test('HS2-6QEZB0 separates local/CI cohorts and preserves fail-fast denominators and reuse', () => {
  const report = buildGateReview(fixture(), { sort: 'name' });
  assert.equal(report.retainedAttempts, 5);
  const local = report.cohorts.find((cohort) => cohort.source === 'local');
  const stage = local.areas.find((area) => area.id === '.').stages.find((item) => item.id === gate);
  assert.equal(stage.label, null);
  assert.deepEqual(
    [stage.metrics.selected, stage.metrics.pass, stage.metrics.fail, stage.metrics.reused],
    [3, 1, 1, 1],
  );
  assert.deepEqual([stage.metrics.cost.p50, stage.metrics.cost.p75, stage.metrics.cost.p90], [100, 300, 300]);
  assert.equal(stage.metrics.failureCategories['exit:1'], 1);
  assert.equal(stage.metrics.trend.comparable, false);
  const missing = local.areas.find((area) => area.id === '.').stages.find((item) => item.id === unreached);
  assert.equal(missing.metrics.selected, 2);
  assert.equal(missing.metrics.deferred, 1);
  assert.equal(missing.metrics.unknown, true);
  assert.equal(local.warmCold, 'unknown');
  assert.equal(local.runnerConcurrency, 'unknown');
  const ci = report.cohorts.find((cohort) => cohort.source === 'ci');
  assert.equal(ci.attempts, 2);
  assert.equal(ci.retries, 1);
  assert.equal(ci.observedRecoveries, 1);
});

test('HS2-6QEZB0 keeps parallel file and case compute separate from gate wall time', () => {
  const report = buildGateReview(fixture(), { source: 'local', area: 'vitest' });
  const vitest = report.cohorts[0].areas[0].stages[0];
  assert.equal(vitest.files[0].metrics.cost.totalMs, 60);
  assert.equal(vitest.files[0].cases[0].metrics.cost.totalMs, 20);
  assert.equal(vitest.metrics.cost.totalMs, 60); // Cases are never added to stage compute.
  assert.equal(
    report.cohorts[0].areas.some((area) => area.id === '.'),
    false,
  );
  assert.match(formatGateReview(report), /may overlap/);
});

test('HS2-6QEZB0 keeps nextest case names under an unknown source file while retaining mapped files', () => {
  const rustFile = 'nextest:crates/hotsheet-cli/tests/cli.rs';
  const rustCase = 'nextest:hotsheet-cli::cli cli::opens_synthetic_project';
  const playwrightFile = 'playwright:tests/synthetic.spec.ts';
  const playwrightCase = `${playwrightFile}#0123456789abcdef`;
  const history = {
    attempts: [
      attempt('runner-shapes', day, [check(rustFile, 'pass', 30), check(playwrightFile, 'pass', 40)], {
        cases: [check(rustCase, 'pass', 10), check(playwrightCase, 'pass', 15)],
      }),
    ],
  };
  const report = buildGateReview(history);
  const areas = report.cohorts[0].areas;
  const nextest = areas.find((item) => item.id === 'nextest').stages[0];
  assert.deepEqual(nextest.files.map((item) => item.id).sort(), [rustFile, 'nextest:unknown-source-file'].sort());
  assert.deepEqual(nextest.files.find((item) => item.id === 'nextest:unknown-source-file').cases.map((item) => item.id), [rustCase]);
  assert.deepEqual(nextest.files.find((item) => item.id === rustFile).cases, []);
  assert.deepEqual(areas.find((item) => item.id === 'playwright').stages[0].files[0].cases.map((item) => item.id), [playwrightCase]);
});

test('HS2-6QEZB0 reports sparse and missing data without fabricated rates or unsafe identifiers', () => {
  const history = fixture();
  history.attempts[0].checks.push(check('/Users/private/name', 'fail', 100, 'secret'));
  history.attempts[0].checks.push(check(gate, 'fail', null, 'customer text'));
  const report = buildGateReview(history, { source: 'local', sort: 'failures', periodDays: 7 });
  assert.equal(JSON.stringify(report).includes('/Users/private'), false);
  assert.equal(JSON.stringify(report).includes('customer text'), false);
  assert.equal(report.cohorts[0].sparse, true);
  assert.equal(
    report.cohorts[0].areas.find((area) => area.id === '.').stages.find((stage) => stage.id === gate).metrics.cost
      .samples,
    2,
  );
  assert.equal(
    report.caveats.some((line) => line.includes('not a representative')),
    true,
  );
});

test('HS2-6QEZB0 compares only observed periods within one source and platform cohort', () => {
  const history = {
    attempts: [
      attempt('a', day, [check(gate, 'pass', 10)]),
      attempt('b', 2 * day, [check(gate, 'pass', 20)]),
      attempt('c', 9 * day, [check(gate, 'pass', 30)]),
      attempt('d', 10 * day, [check(gate, 'pass', 40)]),
      attempt('other-platform', 10 * day, [check(gate, 'fail', 900)], { os: 'linux-x64' }),
    ],
  };
  const report = buildGateReview(history, { os: 'darwin-arm64', periodDays: 7 });
  assert.equal(report.cohorts.length, 1);
  const trend = report.cohorts[0].areas[0].stages.find((stage) => stage.id === gate).metrics.trend;
  assert.equal(trend.comparable, true);
  assert.deepEqual([trend.previous.p50, trend.current.p50], [10, 30]);
  assert.equal(report.cohorts[0].areas[0].stages.find((stage) => stage.id === gate).metrics.fail, 0);
});

test('HS2-6QEZB0 does not call a pass-before-failure sequence a recovery', () => {
  const history = fixture();
  const first = history.attempts.find((attempt) => attempt.id === 'ci:run:1:job:1');
  const second = history.attempts.find((attempt) => attempt.id === 'ci:run:2:job:1');
  first.checks[0].outcome = 'pass';
  second.checks[0].outcome = 'fail';
  assert.equal(buildGateReview(history, { source: 'ci' }).cohorts[0].observedRecoveries, 0);
});

test('HS2-6QEZB0 CLI filters and exports a real bounded ledger without requiring a browser', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'hs2-6qezb0-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const history = join(root, 'history.json');
  writeFileSync(history, JSON.stringify(fixture()));
  assert.equal(parseReviewArgs(['--history', history, '--format', 'json', '--source', 'ci']).source, 'ci');
  assert.throws(() => parseReviewArgs(['--history', history, '--period-days', '0']), /Invalid/);
  const run = spawnSync(
    process.execPath,
    ['scripts/gate-review.mjs', '--history', history, '--format', 'json', '--source', 'ci'],
    { cwd: new URL('..', import.meta.url), encoding: 'utf8' },
  );
  assert.equal(run.status, 0, run.stderr);
  const report = JSON.parse(run.stdout);
  assert.equal(report.cohorts.length, 1);
  assert.equal(report.cohorts[0].source, 'ci');
});
