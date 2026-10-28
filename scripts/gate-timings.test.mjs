import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import {
  appendTiming,
  importCiReports,
  loadTimingHistory,
  recordCompletedGate,
  reportFileTimings,
  stageTimingKey,
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
      const child = spawn(process.execPath, ['--input-type=module', '-e', source, history]);
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
