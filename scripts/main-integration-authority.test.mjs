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
