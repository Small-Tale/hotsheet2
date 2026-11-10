import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { inspectObservations, runGate } from './local-gate.mjs';
import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';
import {
  appendTiming,
  importCiReports,
  importCiJobs,
  loadTimingHistory,
  recordAttempt,
  recordCompletedGate,
  reportCaseObservations,
  reportFileTimings,
  stageTimingKey,
  summarizeObservations,
  summarizeTimings,
  timingEstimate,
} from './gate-timings.mjs';

function fixture(t) {
  const dir = mkdtempSync(join(tmpdir(), 'hs2-tcckrz-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  return { dir, history: join(dir, 'timings.json') };
}

test('HS2-TCCKRZ unknown costs stay unknown and local p75 outranks distinct CI host samples', () => {
  assert.deepEqual(timingEstimate(), { ms: null, samples: 0, host: 'unknown' });
  const entries = [100, 200, 300, 400].map((ms) => ({ ms, host: 'ci' }));
  assert.deepEqual(timingEstimate(entries), { ms: 300, samples: 4, host: 'ci' });
  entries.push({ ms: 150, host: 'local' }, { ms: 250, host: 'local' });
  assert.deepEqual(timingEstimate(entries), { ms: 250, samples: 2, host: 'local' });
});

test('HS2-TCCKRZ history trims each key to a bounded window and rejects invalid samples', (t) => {
  const { history } = fixture(t);
  const data = loadTimingHistory(history);
  for (let ms = 1; ms <= 25; ms += 1) assert.equal(appendTiming(data, 'stages', '.: cargo lint', ms), true);
  assert.equal(appendTiming(data, 'stages', 'invalid', 0), false);
  assert.equal(data.stages['.: cargo lint'].length, 20);
  assert.equal(data.stages['.: cargo lint'][0].ms, 6);
  assert.deepEqual(summarizeTimings(data, { stages: ['missing'], files: ['missing'] }), {
    stages: [{ key: 'missing', ms: null, samples: 0, host: 'unknown' }],
    files: [{ key: 'missing', ms: null, samples: 0, host: 'unknown' }],
  });
});

test('HS2-TCCKRZ only completed executed stages enter timing history', (t) => {
  const { history } = fixture(t);
  recordCompletedGate(history, [
    {
      cwd: '.',
      command: 'cargo lint',
      ms: 1200,
      status: 0,
      files: [{ key: 'nextest:crates/hotsheet-cli/tests/cli.rs', ms: 400 }],
    },
    { cwd: '.', command: 'cargo fmt', ms: 0, status: 0 },
    { cwd: '.', command: 'reused', ms: 0, status: 0, reused: true },
    { cwd: '.', command: 'failed', ms: 500, status: 1 },
  ]);
  const data = loadTimingHistory(history);
  assert.equal(stageTimingKey({ cwd: '.', command: 'cargo lint' }), '.: cargo lint');
  assert.deepEqual(Object.keys(data.stages), ['.: cargo lint']);
  assert.equal(data.files['nextest:crates/hotsheet-cli/tests/cli.rs'][0].ms, 400);
  assert.equal(JSON.parse(readFileSync(history, 'utf8')).version, 1);
});

test('HS2-TCCKRZ parses actual runner report shapes into safe file and binary keys', (t) => {
  const { dir } = fixture(t);
  const report = (name) => readFileSync(new URL(`./fixtures/test-profile/${name}`, import.meta.url), 'utf8');
  mkdirSync(join(dir, 'crates/hotsheet-plugins/src'), { recursive: true });
  writeFileSync(join(dir, 'crates/hotsheet-plugins/src/setup.rs'), '');
  assert.ok(
    reportFileTimings('nextest', report('nextest-junit.xml'), dir).some(
      (row) => row.key === 'nextest:crates/hotsheet-plugins/src/setup.rs',
    ),
  );
  assert.ok(reportFileTimings('vitest', report('vitest.json'), '/root').some((row) => row.key.startsWith('vitest:')));
  assert.ok(
    reportFileTimings('playwright', report('playwright.json'), dir).some((row) => row.key.startsWith('playwright:')),
  );
  assert.deepEqual(
    reportFileTimings(
      'vitest',
      JSON.stringify({
        testResults: [{ name: '/ci-root/clients/web/src/ci.test.ts', startTime: 0, endTime: 1000 }],
      }),
      dir,
    ),
    [{ key: 'vitest:src/ci.test.ts', ms: 1000 }],
  );
  assert.ok(
    reportFileTimings(
      'vitest',
      JSON.stringify({ testResults: [{ name: '/etc/private', startTime: 0, endTime: 1000 }] }),
      dir,
    ).length === 0,
  );
});

test('HS2-TCCKRZ CI import seeds missing local estimates with separate host provenance', (t) => {
  const { dir, history } = fixture(t);
  const vitest = join(dir, 'vitest.json');
  writeFileSync(
    vitest,
    JSON.stringify({
      success: true,
      testResults: [{ name: join(dir, 'clients/web/src/example.test.ts'), startTime: 0, endTime: 1200 }],
    }),
  );
  importCiReports(history, { vitest }, { vitest: join(dir, 'clients/web') });
  const data = loadTimingHistory(history);
  assert.deepEqual(timingEstimate(data.files['vitest:src/example.test.ts']), { ms: 1200, samples: 1, host: 'ci' });
  importCiReports(history, { vitest }, { vitest: join(dir, 'clients/web') });
  assert.equal(loadTimingHistory(history).files['vitest:src/example.test.ts'].length, 1);
  appendTiming(data, 'files', 'vitest:src/example.test.ts', 900, 'local');
  assert.deepEqual(timingEstimate(data.files['vitest:src/example.test.ts']), { ms: 900, samples: 1, host: 'local' });
  writeFileSync(
    vitest,
    JSON.stringify({
      success: false,
      testResults: [{ name: join(dir, 'clients/web/src/failed.test.ts'), startTime: 0, endTime: 1200 }],
    }),
  );
  importCiReports(history, { vitest }, { vitest: join(dir, 'clients/web') });
  assert.equal(loadTimingHistory(history).files['vitest:src/failed.test.ts'], undefined);
  const nextest = join(dir, 'nextest-junit.xml');
  writeFileSync(
    nextest,
    '<testsuite failures="1"><testcase name="setup::tests::fails" classname="hotsheet-plugins" time="1"/><failure/></testsuite>',
  );
  mkdirSync(join(dir, 'crates/hotsheet-plugins/src'), { recursive: true });
  writeFileSync(join(dir, 'crates/hotsheet-plugins/src/setup.rs'), '');
  importCiReports(history, { nextest }, { nextest: dir });
  assert.equal(loadTimingHistory(history).files['nextest:crates/hotsheet-plugins/src/setup.rs'], undefined);
});

test('HS2-TCCKRZ concurrent workers retain both timing samples', async (t) => {
  const { history } = fixture(t);
  const moduleUrl = new URL('./gate-timings.mjs', import.meta.url).href;
  const source = `import { recordCompletedGate } from ${JSON.stringify(moduleUrl)}; recordCompletedGate(process.argv[1], [{ cwd: '.', command: 'cargo lint', status: 0, ms: 100 }]);`;
  const run = () =>
    new Promise((resolve, reject) => {
      const child = spawn(process.execPath, ['--input-type=module', '-e', source, history], { env: withoutGitRepositoryEnv(process.env) });
      let stderr = '';
      child.stderr.on('data', (chunk) => {
        stderr += chunk;
      });
      child.on('error', reject);
      child.on('exit', (status) => (status === 0 ? resolve() : reject(new Error(stderr))));
    });
  await Promise.all([run(), run()]);
  assert.equal(loadTimingHistory(history).stages['.: cargo lint'].length, 2);
});

test('HS2-5RDMEV records failed, interrupted, reused and deferred checks without false fast passes', (t) => {
  const { history } = fixture(t);
  const base = { id: 'local:run-1', source: 'local', cohort: 'local-T2', startedAt: 100,
    selected: ['rust', 'web'], deferred: ['browser'], checks: [
      { id: 'rust', outcome: 'pass', startedAt: 110, endedAt: 210, ms: 100 },
      { id: 'web', outcome: 'interrupted', startedAt: 220 },
    ] };
  recordAttempt(history, base);
  recordAttempt(history, { ...base, endedAt: 300, checks: [
    base.checks[0], { id: 'web', outcome: 'fail', startedAt: 220, endedAt: 300, ms: 80, category: 'exit:1' },
  ] });
  recordAttempt(history, { id: 'local:run-2', source: 'local', selected: ['rust'], deferred: ['web', 'browser'],
    checks: [{ id: 'rust', outcome: 'reused' }] });
  const summary = summarizeObservations(loadTimingHistory(history), ['missing']);
  assert.equal(summary.attempts, 2);
  assert.deepEqual(summary.checks.find((row) => row.id === 'web'), {
    id: 'web', selected: 1, deferred: 1, attempted: 1, pass: 0, fail: 1, skip: 0, reused: 0,
    interrupted: 0, successMs: { samples: 0, p50: null, p75: null, p90: null },
    failureMs: { samples: 1, p50: 80, p75: 80, p90: 80 }, unknown: false,
  });
  assert.equal(summary.checks.find((row) => row.id === 'missing').unknown, true);
  assert.equal(summary.checks.find((row) => row.id === 'rust').reused, 1);
  assert.deepEqual(summary.cohorts.map((row) => [row.cohort, row.attempts]), [['local', 1], ['local-T2', 1]]);
  assert.equal(summary.cohorts.find((row) => row.cohort === 'local-T2').checks.find((row) => row.id === 'rust').reused, 0);
});

test('HS2-5RDMEV bounds attempts and rejects path or raw diagnostic identities', (t) => {
  const { history } = fixture(t);
  for (let index = 0; index < 25; index += 1) recordAttempt(history, {
    id: `local:${index}`, source: 'local', selected: ['check'], checks: [{ id: 'check', outcome: 'fail', category: 'exit:1' }],
  });
  assert.equal(loadTimingHistory(history).attempts.length, 20);
  assert.throws(() => recordAttempt(history, { id: 'local:private', source: 'local', selected: ['/Users/private/secret'], checks: [] }), /Invalid/);
  assert.throws(() => recordAttempt(history, { id: 'local:private', source: 'local', selected: [], checks: [{ id: 'x', outcome: 'raw error: /Users/private' }] }), /Invalid/);
});

test('HS2-5RDMEV imports failed CI case observations and deduplicates run attempts', (t) => {
  const { dir, history } = fixture(t);
  const file = join(dir, 'vitest.json');
  writeFileSync(file, JSON.stringify({ success: false, testResults: [{ name: join(dir, 'clients/web/src/a.test.ts'),
    startTime: 0, endTime: 100, assertionResults: [{ fullName: 'dynamic private title', status: 'failed', duration: 40, failureMessages: ['secret'] }] }] }));
  const roots = { vitest: join(dir, 'clients/web') };
  const meta = { runId: '123', runAttempt: '2', cohort: 'ci-main', os: 'Linux-X64' };
  importCiReports(history, { vitest: file }, roots, meta);
  importCiReports(history, { vitest: file }, roots, meta);
  const jobs = { jobs: [{ id: 7, name: 'test', started_at: '2026-11-10T00:00:00Z', completed_at: '2026-11-10T00:00:02Z', steps: [
    { name: 'web', started_at: '2026-11-10T00:00:00Z', completed_at: '2026-11-10T00:00:02Z', conclusion: 'failure' },
    { name: 'browser', conclusion: 'skipped' },
  ] }] };
  importCiJobs(history, jobs, meta);
  importCiJobs(history, jobs, meta);
  const data = loadTimingHistory(history);
  assert.equal(data.attempts.length, 2);
  assert.equal(data.attempts[0].cases[0].outcome, 'fail');
  assert.equal(data.attempts[0].host, 'ci-runner');
  assert.equal(data.attempts[0].cases[0].id.includes('dynamic private title'), false);
  assert.equal(JSON.stringify(data).includes('secret'), false);
  assert.equal(data.attempts[1].checks[0].ms, 2000);
  assert.equal(data.attempts[1].checks[1].outcome, 'skip');
  assert.deepEqual(data.attempts[1].deferred, ['ci:test:browser']);
  assert.equal(reportCaseObservations('vitest', readFileSync(file, 'utf8'), roots.vitest).length, 1);
  assert.equal(reportCaseObservations('vitest', JSON.stringify({ testResults: [{ name: 'C:\\ci\\clients\\web\\src\\win.test.ts', assertionResults: [{ fullName: 'works', status: 'passed', duration: 2 }] }] }), roots.vitest)[0].id.startsWith('vitest:src/win.test.ts#'), true);
});

test('HS2-5RDMEV real gate records failed execution, passing retry, and receipt reuse without changing status', (t) => {
  const { dir } = fixture(t);
  const git = (...args) => execFileSync('git', args, { cwd: dir, env: withoutGitRepositoryEnv(process.env), encoding: 'utf8' }).trim();
  const put = (file, body) => { mkdirSync(dirname(join(dir, file)), { recursive: true }); writeFileSync(join(dir, file), body); };
  git('init', '--quiet', '-b', 'main');
  git('config', 'user.name', 'Synthetic');
  git('config', 'user.email', 'synthetic@invalid.example');
  put('scripts/check-full.sh', readFileSync(new URL('./check-full.sh', import.meta.url), 'utf8'));
  put('scripts/gate-budget.json', JSON.stringify({ defaultBudgetMs: 300000 }));
  put('clients/web/src/example.ts', 'export const value = 1;\n');
  git('add', '.'); git('commit', '--quiet', '-m', 'base'); git('update-ref', 'refs/remotes/origin/main', 'HEAD');
  put('clients/web/src/example.ts', 'export const value = 2;\n');
  git('commit', '--quiet', '-am', 'change');
  const options = { repo: dir, versions: [['synthetic-toolchain', '1']], log: () => {} };
  assert.equal(runGate({ ...options, runner: () => ({ status: 7 }) }), 7);
  assert.equal(runGate({ ...options, runner: () => ({ status: 0 }) }), 0);
  assert.equal(runGate({ ...options, runner: () => { throw new Error('receipt should be reused'); } }), 0);
  const data = loadTimingHistory(join(dir, '.git/hotsheet-gate-timings.json'));
  assert.equal(data.attempts.length, 3);
  assert.deepEqual(data.attempts.map((entry) => entry.checks[0].outcome), ['fail', 'pass', 'reused']);
  assert.equal(data.attempts[0].checks.length, 1, 'first failure still stops');
  assert.equal(data.attempts[2].checks[0].ms, null, 'receipt reuse has no synthetic duration');
  assert.equal(inspectObservations(dir).attempts, 3);
});
