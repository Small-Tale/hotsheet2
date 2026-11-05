import assert from 'node:assert/strict';
import test from 'node:test';
import { parseMainPushArgs } from './main-integration-options.mjs';

test('HS2-BND42N freezes one named remote and main target', () => {
  assert.deepEqual(parseMainPushArgs([]).remote, 'origin');
  assert.deepEqual(parseMainPushArgs(['--atomic', 'origin', 'HEAD:main']).target, 'refs/heads/main');
  assert.deepEqual(parseMainPushArgs(['origin', 'main']).source, 'main');
  assert.deepEqual(parseMainPushArgs(['origin', 'main:refs/heads/main', '--porcelain']).gitFlags, ['--porcelain']);
});

test('HS2-BND42N forwards only analyzed gate options and exact pool identity', () => {
  const parsed = parseMainPushArgs([
    '--integrator-id',
    'owner',
    '--integrator-session',
    'generation',
    '--assignment',
    'handoff',
    '--gate-arg',
    '--ticket',
    '--gate-arg',
    'HS2-BND42N',
    '--gate-arg',
    '--full',
  ]);
  assert.deepEqual(parsed.identity, { id: 'owner', session: 'generation' });
  assert.deepEqual(parsed.gateArgs, ['--ticket', 'HS2-BND42N', '--full']);
  assert.equal(parsed.assignment, 'handoff');
});

test('HS2-BND42N rejects force, deletion, URL, arbitrary ref and receipt bypass', () => {
  for (const args of [
    ['--force'],
    ['--delete'],
    ['--all'],
    ['--mirror'],
    ['--dry-run'],
    ['https://example.invalid/repo'],
    ['origin', ':main'],
    ['origin', 'HEAD:other'],
    ['origin', 'other:main'],
    ['origin', 'HEAD:main', 'other:main'],
    ['--source', 'HEAD', 'origin', 'main'],
    ['--assignment', 'handoff'],
    ['--integrator-id', 'owner'],
    ['--standalone', '--integrator-id', 'owner', '--integrator-session', 'gen'],
    ['--gate-arg', '--plan'],
    ['--gate-arg', '--verify'],
    ['--gate-arg', '--ticket'],
    ['--gate-arg', '--ticket', '--gate-arg', 'one', '--gate-arg', '--ticket', '--gate-arg', 'two'],
  ])
    assert.throws(() => parseMainPushArgs(args), args.join(' '));
});

test('HS2-9G0M3V a batch push names every member, its phase tickets and only the smoke gate', () => {
  const identity = ['--integrator-id', 'owner', '--integrator-session', 'generation'];
  const parsed = parseMainPushArgs([
    ...identity,
    '--assignment',
    'op-1',
    '--assignment',
    'op-2',
    '--phase-ticket',
    'HS2-AAA111',
    '--phase-ticket',
    'HS2-BBB222',
    '--gate-arg',
    '--smoke',
  ]);
  assert.deepEqual(parsed.assignments, ['op-1', 'op-2']);
  assert.equal(parsed.assignment, 'op-1');
  assert.deepEqual(parsed.phaseTickets, ['HS2-AAA111', 'HS2-BBB222']);
  assert.deepEqual(parsed.gateArgs, ['--smoke']);
  assert.deepEqual(parseMainPushArgs([...identity, '--assignment', 'op-1']).assignments, ['op-1']);
  for (const args of [
    [...identity, '--assignment', 'op-1', '--assignment', 'op-2'],
    [...identity, '--assignment', 'op-1', '--assignment', 'op-1', '--gate-arg', '--smoke'],
    ['--phase-ticket', 'HS2-AAA111'],
    ['--gate-arg', '--smoke', '--gate-arg', '--full'],
    ['--gate-arg', '--smoke', '--gate-arg', '--ticket', '--gate-arg', 'HS2-AAA111'],
    ['--gate-arg', '--smoke', '--gate-arg', '--t2-stage', '--gate-arg', '.::cargo lint'],
    ['--gate-arg', '--smoke', '--gate-arg', '--smoke'],
  ])
    assert.throws(() => parseMainPushArgs(args), args.join(' '));
});

test('HS2-GT57BQ staged and promote pushes name the integration branch or one exact SHA', () => {
  const identity = ['--integrator-id', 'i', '--integrator-session', 'g'];
  const staged = parseMainPushArgs(['--staged', ...identity, '--gate-arg', '--smoke']);
  assert.equal(staged.mode, 'staged');
  assert.equal(staged.target, 'refs/heads/integration');
  assert.equal(
    parseMainPushArgs(['--staged', '--staging-branch', 'staging/ci', ...identity]).target,
    'refs/heads/staging/ci',
  );
  for (const branch of ['main', 'HEAD', 'a..b', 'x/', 'x.lock', '-x', 'a//b'])
    assert.throws(() => parseMainPushArgs(['--staged', '--staging-branch', branch, ...identity]), /Invalid staging/);
  assert.throws(() => parseMainPushArgs(['--staging-branch', 'integration', ...identity]), /requires --staged/);
  assert.throws(() => parseMainPushArgs(['--staged', 'origin', 'HEAD:main', ...identity]), /omit the refspec/);
  assert.throws(() => parseMainPushArgs(['--staged']), /requires integrator identity/);
  const sha = 'A'.repeat(40);
  const promote = parseMainPushArgs(['--promote', sha, ...identity]);
  assert.deepEqual([promote.mode, promote.source, promote.target], ['promote', 'a'.repeat(40), 'refs/heads/main']);
  assert.throws(() => parseMainPushArgs(['--promote', 'abc', ...identity]), /full green staged SHA/);
  assert.throws(() => parseMainPushArgs(['--promote', sha, '--staged', ...identity]), /cannot be combined/);
  assert.throws(() => parseMainPushArgs(['--promote', sha, '--gate-arg', '--smoke', ...identity]), /no local gate/);
  assert.throws(() => parseMainPushArgs(['--promote', sha, '--assignment', 'op', ...identity]), /moves no assignment/);
  assert.throws(() => parseMainPushArgs(['--promote', sha, '--source', 'HEAD', ...identity]), /omit --source/);
  assert.throws(() => parseMainPushArgs(['--promote', sha]), /requires integrator identity/);
});
