import assert from 'node:assert/strict';
import test from 'node:test';
import { authorizeMainState } from './main-integration-window.mjs';

const project = 'checkout-123456789abc';
const source = 'a'.repeat(64);
const now = 1000;
const value = { identity: null, assignment: null, standalone: false };
const state = () => ({
  version: 6,
  project_id: project,
  source_id: source,
  stopped: null,
  integrator: null,
  assignments: {},
});
const authorize = (pool, request = value) => authorizeMainState(pool, request, project, source, now);

test('HS2-BND42N absent and retained pools have explicit standalone rules', () => {
  assert.equal(authorize(null), 'standalone');
  assert.throws(() => authorize(null, { ...value, identity: { id: 'i', session: 'g' } }));
  assert.throws(() => authorize(state()), /explicit --standalone/);
  assert.equal(authorize(state(), { ...value, standalone: true }), 'standalone');
  assert.throws(() => authorize({ ...state(), stopped: 'drained' }), /explicit --standalone/);
  assert.equal(authorize({ ...state(), stopped: 'drained' }, { ...value, standalone: true }), 'standalone');
});

test('HS2-BND42N exact live integrator generation and code handoff fence push', () => {
  const pool = state();
  pool.integrator = { id: 'i', generation: 'g', expires_at: now + 1 };
  const owner = { ...value, identity: { id: 'i', session: 'g' } };
  assert.equal(authorize(pool, owner), 'integrator');
  assert.throws(() => authorize(pool, value), /Live integrator/);
  assert.throws(() => authorize(pool, { ...owner, identity: { id: 'i', session: 'old' } }), /Live integrator/);
  pool.assignments.handoff = { id: 'handoff', phase: 'integrating', handoff: { kind: 'code', commit: 'b'.repeat(40) } };
  assert.throws(() => authorize(pool, owner), /Specify the active/);
  assert.equal(authorize(pool, { ...owner, assignment: 'handoff' }), 'integrator');
  assert.throws(() => authorize(pool, { ...owner, assignment: 'wrong' }), /sole integrating code/);
  pool.assignments.handoff.phase = 'verifying';
  assert.throws(() => authorize(pool, { ...owner, assignment: 'handoff' }), /sole integrating code/);
});

test('HS2-BND42N expired review fence and incompatible source fail closed', () => {
  const pool = state();
  pool.integrator = { id: 'i', generation: 'g', expires_at: now - 1 };
  pool.assignments.handoff = { id: 'handoff', phase: 'integrating', commit: 'b'.repeat(40) };
  assert.throws(() => authorize(pool, { ...value, standalone: true }), /needs recovery/);
  assert.throws(() => authorize(pool, { ...value, identity: { id: 'i', session: 'g' } }), /expired/);
  pool.assignments = {};
  assert.equal(authorize(pool, { ...value, standalone: true }), 'standalone');
  pool.source_id = 'wrong';
  assert.throws(() => authorize(pool, { ...value, standalone: true }), /Incompatible/);
  pool.source_id = source;
  pool.version = 2;
  assert.throws(() => authorize(pool, { ...value, standalone: true }), /Incompatible/);
});

test('HS2-9G0M3V a batch push needs a batch integrator and exactly the integrating members', () => {
  const pool = { ...state(), version: 8 };
  pool.integrator = { id: 'i', generation: 'g', expires_at: now + 1, strategy: 'batch' };
  const code = (id) => ({ id, phase: 'integrating', handoff: { kind: 'code', commit: 'b'.repeat(40) } });
  pool.assignments = { 'op-1': code('op-1'), 'op-2': code('op-2') };
  const owner = { ...value, identity: { id: 'i', session: 'g' }, gateArgs: ['--smoke'] };
  const batch = { ...owner, assignment: 'op-1', assignments: ['op-1', 'op-2'] };
  assert.equal(authorize(pool, batch), 'integrator');
  assert.throws(() => authorize(pool, { ...batch, assignments: ['op-1', 'op-3'] }), /exactly the integrating/);
  pool.assignments['op-3'] = code('op-3');
  assert.throws(() => authorize(pool, batch), /exactly the integrating/);
  delete pool.assignments['op-3'];
  pool.assignments['op-2'].phase = 'ci';
  assert.throws(() => authorize(pool, batch), /exactly the integrating/);
  pool.assignments['op-2'].phase = 'integrating';
  // Schemas seven through nine are read; ten is not.
  pool.version = 10;
  assert.throws(() => authorize(pool, batch), /Incompatible/);
  pool.version = 7;
  pool.integrator.strategy = undefined;
  assert.throws(() => authorize(pool, batch), /only for a batch integrator/);
  assert.throws(() => authorize(pool, { ...batch, gateArgs: [] }), /needs a batch integrator/);
  // The smoke gate never applies to a standalone push.
  assert.throws(() => authorize(null, { ...value, gateArgs: ['--smoke'] }), /only for a batch/);
  assert.throws(
    () => authorize({ ...state(), version: 8 }, { ...value, standalone: true, gateArgs: ['--smoke'] }),
    /only for a batch/,
  );
});

test('HS2-JBAD2Z the push wrapper reads a schema-nine pool with worker preferences', () => {
  // HS2-6WFCZR persists schema nine for worker preferences; they live on worker sessions,
  // which the wrapper does not read, so a schema-nine batch pool authorizes unchanged.
  const pool = { ...state(), version: 9 };
  pool.integrator = { id: 'i', generation: 'g', expires_at: now + 1, strategy: 'batch' };
  pool.workers = {
    w: { id: 'w', generation: 'wg', expires_at: now + 1, preferences: { only: ['tag:glance'], request: 'HS2-AAAAAA' } },
  };
  const code = (id) => ({ id, phase: 'integrating', handoff: { kind: 'code', commit: 'b'.repeat(40) } });
  pool.assignments = { 'op-1': code('op-1'), 'op-2': code('op-2') };
  const batch = {
    ...value,
    identity: { id: 'i', session: 'g' },
    gateArgs: ['--smoke'],
    assignment: 'op-1',
    assignments: ['op-1', 'op-2'],
  };
  assert.equal(authorize(pool, batch), 'integrator');
  pool.integrator = { id: 'i', generation: 'g', expires_at: now + 1 };
  pool.assignments = { 'op-1': code('op-1') };
  assert.equal(authorize(pool, { ...value, identity: { id: 'i', session: 'g' }, assignment: 'op-1' }), 'integrator');
  pool.version = 10;
  assert.throws(() => authorize(pool, batch), /Incompatible/);
});
