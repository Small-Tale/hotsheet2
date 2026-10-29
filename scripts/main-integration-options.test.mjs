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
