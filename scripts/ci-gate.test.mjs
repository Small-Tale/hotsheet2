import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  gateStatus,
  isCovered,
  latestGreenRun,
  parseStatusArgs,
  startedSlugs,
  streamWaitingCommit,
  waitingCommit,
} from './ci-gate.mjs';

const oldSha = 'a'.repeat(40);
const greenSha = 'b'.repeat(40);
const futureSha = 'c'.repeat(40);
const recordedRuns = [
  {
    conclusion: 'success',
    status: 'completed',
    headBranch: 'other',
    event: 'push',
    workflowName: 'CI',
    headSha: futureSha,
    databaseId: 3,
    createdAt: '2026-10-17T09:00:00Z',
    url: 'https://example.test/3',
  },
  {
    conclusion: 'success',
    status: 'completed',
    headBranch: 'main',
    event: 'push',
    workflowName: 'CI',
    headSha: greenSha,
    databaseId: 2,
    createdAt: '2026-10-17T08:00:00Z',
    url: 'https://example.test/2',
  },
  {
    conclusion: 'success',
    status: 'completed',
    headBranch: 'main',
    event: 'workflow_dispatch',
    workflowName: 'CI',
    headSha: futureSha,
    databaseId: 4,
    createdAt: '2026-10-17T10:00:00Z',
    url: 'https://example.test/4',
  },
  {
    conclusion: 'success',
    status: 'completed',
    headBranch: 'main',
    event: 'push',
    workflowName: 'CI',
    headSha: oldSha,
    databaseId: 1,
    createdAt: '2026-10-17T07:00:00Z',
    url: 'https://example.test/1',
  },
];

test('selects the latest successful main push CI run, regardless of input order', () => {
  assert.equal(latestGreenRun(recordedRuns)?.headSha, greenSha);
  assert.equal(latestGreenRun([]), null);
  assert.throws(() => latestGreenRun({}), /non-array/);
});

test('HS2-WSE3FJ never counts a daily, manual or called platform run as a per-commit gate', () => {
  const platformRuns = ['schedule', 'workflow_dispatch', 'push'].map((event, index) => ({
    conclusion: 'success',
    status: 'completed',
    headBranch: 'main',
    event,
    workflowName: 'Platforms',
    headSha: futureSha,
    databaseId: 10 + index,
    createdAt: `2026-11-03T0${index}:00:00Z`,
    url: `https://example.test/${10 + index}`,
  }));
  assert.equal(latestGreenRun(platformRuns), null);
  assert.equal(latestGreenRun([...platformRuns, ...recordedRuns])?.headSha, greenSha);
  const scheduledCi = { ...recordedRuns[1], event: 'schedule', headSha: futureSha, createdAt: '2026-11-03T09:00:00Z' };
  assert.equal(latestGreenRun([scheduledCi, ...recordedRuns])?.headSha, greenSha);
});

test('reads only listed ticket slugs and final-testing note markers', () => {
  assert.deepEqual(startedSlugs('*● HS2-ABC started title mentions HS2-OTHER\n● HS2-DEF started title\n'), [
    'HS2-ABC',
    'HS2-DEF',
  ]);
  const ticket = (phase, markers) =>
    `status: started\nstarted_phase: ${phase}\n<!-- hotsheet:notes:begin -->\n${markers}\n`;
  assert.equal(
    waitingCommit(ticket('final_testing', `CI gate commit: ${oldSha}\nCI gate commit: ${greenSha}`)),
    greenSha,
  );
  assert.equal(waitingCommit(ticket('working', `CI gate commit: ${greenSha}`)), null);
  assert.equal(
    waitingCommit(
      `status: completed\nstarted_phase: final_testing\n<!-- hotsheet:notes:begin -->\nCI gate commit: ${greenSha}`,
    ),
    null,
  );
  assert.equal(waitingCommit(ticket('final_testing', 'No marker')), null);
  assert.equal(
    waitingCommit(
      `---\nstatus: started\nstarted_phase: working\n---\n<!-- hotsheet:body:begin -->\nstarted_phase: final_testing\n<!-- hotsheet:notes:begin -->\nCI gate commit: ${greenSha}\n`,
    ),
    null,
  );
});

test('coverage distinguishes ancestor, non-ancestor, and missing local objects', () => {
  const run = (_binary, args) => ({ status: args[0] === 'merge-base' ? 0 : 0 });
  assert.equal(isCovered(oldSha, greenSha, run), 'covered');
  assert.equal(
    isCovered(oldSha, greenSha, (_binary, args) => ({ status: args[0] === 'merge-base' ? 1 : 0 })),
    'uncovered',
  );
  assert.equal(
    isCovered(oldSha, greenSha, (_binary, args) => ({
      status: args[0] === 'cat-file' && args[2].startsWith(greenSha) ? 1 : 0,
    })),
    'unknown',
  );
  assert.throws(() => isCovered('short', greenSha, run), /full commit SHAs/);
});

test('HS2-HBGGRQ streams a large ticket without losing its final CI marker', async () => {
  const script = `
    process.stdout.write('status: started\\nstarted_phase: final_testing\\n');
    process.stdout.write('<!-- hotsheet:notes:begin -->\\n');
    process.stdout.write('x'.repeat(2 * 1024 * 1024) + '\\n');
    process.stdout.write('CI gate commit: ${greenSha}\\n');
  `;
  assert.equal(await streamWaitingCommit(process.execPath, ['-e', script]), greenSha);
  await assert.rejects(
    streamWaitingCommit(process.execPath, ['-e', "process.stderr.write('synthetic failure'); process.exit(2)"]),
    /synthetic failure/,
  );
});

test('reports only explicit final-testing tickets covered by the green run', async () => {
  const calls = [];
  const run = (binary, args) => {
    calls.push([binary, ...args]);
    if (binary === 'gh') return { status: 0, stdout: JSON.stringify(recordedRuns) };
    if (binary === 'hotsheet-cli' && args[0] === 'ls') {
      return { status: 0, stdout: '*● HS2-ONE started first\n● HS2-TWO started second\n' };
    }
    if (args[0] === 'cat-file') return { status: 0 };
    if (args[0] === 'merge-base') return { status: args[2] === futureSha ? 1 : 0 };
    throw new Error(`unexpected command: ${binary} ${args.join(' ')}`);
  };
  const readTicket = async (slug) => {
    const marker = slug === 'HS2-ONE' ? oldSha : futureSha;
    return waitingCommit(
      `status: started\nstarted_phase: final_testing\n<!-- hotsheet:notes:begin -->\nCI gate commit: ${marker}\n`,
    );
  };
  const result = await gateStatus({ covers: oldSha, run, readTicket });
  assert.equal(result.green.headSha, greenSha);
  assert.equal(result.covers, 'covered');
  assert.deepEqual(result.waiting, [
    { slug: 'HS2-ONE', commit: oldSha, coverage: 'covered' },
    { slug: 'HS2-TWO', commit: futureSha, coverage: 'uncovered' },
  ]);
  assert.ok(calls.some((call) => call[0] === 'gh' && call.includes('--branch') && call.includes('main')));
});

test('HS2-GT57BQ status reads the covering run on a ci-staged integration branch', async () => {
  const staged = recordedRuns.map((run) => (run.headBranch === 'other' ? { ...run, headBranch: 'integration' } : run));
  assert.equal(latestGreenRun(staged, 'integration')?.headSha, futureSha);
  assert.equal(latestGreenRun(staged)?.headSha, greenSha);
  const calls = [];
  const run = (binary, args) => {
    calls.push([binary, ...args]);
    if (binary === 'gh') return { status: 0, stdout: JSON.stringify(staged) };
    if (binary === 'hotsheet-cli') return { status: 0, stdout: '' };
    if (args[0] === 'cat-file' || args[0] === 'merge-base') return { status: 0 };
    throw new Error(`unexpected command: ${binary} ${args.join(' ')}`);
  };
  const result = await gateStatus({ covers: futureSha, branch: 'integration', run, readTicket: async () => null });
  assert.equal(result.green.headSha, futureSha);
  assert.equal(result.covers, 'covered');
  const listed = calls.find((call) => call[0] === 'gh');
  assert.equal(listed[listed.indexOf('--branch') + 1], 'integration');
  await assert.rejects(gateStatus({ branch: 'a..b', run }), /plain branch/);
  assert.deepEqual(parseStatusArgs(['status']), { branch: 'main', covers: undefined });
  assert.deepEqual(parseStatusArgs(['status', '--branch', 'integration', '--covers', futureSha]), {
    branch: 'integration',
    covers: futureSha,
  });
  for (const args of [
    ['status', '--covers', 'abc'],
    ['status', '--branch'],
    ['status', '--branch', 'x', '--branch', 'y'],
    ['status', '--other', 'x'],
    ['list'],
  ])
    assert.throws(() => parseStatusArgs(args), /usage/);
});
