import assert from 'node:assert/strict';
import { test } from 'node:test';

import { gateStatus, isCovered, latestGreenRun, startedSlugs, waitingCommit } from './ci-gate.mjs';

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

test('reports only explicit final-testing tickets covered by the green run', () => {
  const calls = [];
  const run = (binary, args) => {
    calls.push([binary, ...args]);
    if (binary === 'gh') return { status: 0, stdout: JSON.stringify(recordedRuns) };
    if (binary === 'hotsheet-cli' && args[0] === 'ls') {
      return { status: 0, stdout: '*● HS2-ONE started first\n● HS2-TWO started second\n' };
    }
    if (binary === 'hotsheet-cli') {
      const marker = args[1] === 'HS2-ONE' ? oldSha : futureSha;
      return {
        status: 0,
        stdout: `status: started\nstarted_phase: final_testing\n<!-- hotsheet:notes:begin -->\nCI gate commit: ${marker}\n`,
      };
    }
    if (args[0] === 'cat-file') return { status: 0 };
    if (args[0] === 'merge-base') return { status: args[2] === futureSha ? 1 : 0 };
    throw new Error(`unexpected command: ${binary} ${args.join(' ')}`);
  };
  const result = gateStatus({ covers: oldSha, run });
  assert.equal(result.green.headSha, greenSha);
  assert.equal(result.covers, 'covered');
  assert.deepEqual(result.waiting, [
    { slug: 'HS2-ONE', commit: oldSha, coverage: 'covered' },
    { slug: 'HS2-TWO', commit: futureSha, coverage: 'uncovered' },
  ]);
  assert.ok(calls.some((call) => call[0] === 'gh' && call.includes('--branch') && call.includes('main')));
});
