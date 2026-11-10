import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import {
  planIdentity,
  planEstimate,
  rememberTestPlan,
  storedTestPlan,
  testPlanPath,
  validateTestPlan,
  verifyTestPlanSelection,
} from './gate-test-plan.mjs';

const profile = { ticket: 'conn:native', developmentStrategy: { preset: 'risk-guided', source: 'ticket' } };
const state = { tree: 'tree-a', base: 'base-a', changes: ['a', 'b'], profile };
const manifest = () => ({
  version: 1,
  ticket: profile.ticket,
  tree: state.tree,
  base: state.base,
  changed_paths: ['a', 'b'],
  strategy: 'risk-guided',
  strategy_source: 'ticket',
  behavior: 'Change a state transition.',
  risks: ['Concurrent state update'],
  reviewed_by: 'synthetic reviewer',
  analyzed: ['.::node --test'],
  proposed: [],
  selected: [{ id: '.::node --test', reason: 'Direct regression', estimate_ms: 1200 }],
  deferred: [
    { id: 'clients/web::npm run test:browser', reason: 'Unaffected UI', ci_coverage: 'main CI browser shard' },
  ],
});
const prepared = {
  checks: [{ cwd: '.', command: 'node --test' }],
  stages: [
    { cwd: '.', command: 'node --test', selected: true },
    { cwd: 'clients/web', command: 'npm run test:browser', selected: false },
  ],
};

test('HS2-RRNZYN persists an exact-tree reviewed plan and binds receipt selection', (t) => {
  const cache = mkdtempSync(join(tmpdir(), 'hs2-rrnzyn-'));
  t.after(() => rmSync(cache, { recursive: true, force: true }));
  const plan = validateTestPlan(manifest(), state);
  verifyTestPlanSelection(plan, prepared);
  rememberTestPlan(cache, plan);
  assert.deepEqual(storedTestPlan(cache, plan), plan);
  assert.equal(storedTestPlan(cache, { ...plan, tree: 'tree-b' }), null);
  assert.equal(planIdentity(plan), planIdentity(JSON.parse(readFileSync(testPlanPath(cache, plan), 'utf8'))));
  assert.deepEqual(planEstimate(plan), { known_ms: 1200, unknown_checks: 0 });
  const other = { ...plan, ticket: 'other:native' };
  rememberTestPlan(cache, other);
  assert.deepEqual(storedTestPlan(cache, plan), plan, 'another ticket on the same tree does not overwrite this plan');
  assert.deepEqual(storedTestPlan(cache, other), other);
  writeFileSync(`${testPlanPath(cache, plan)}.interrupted.tmp`, '{');
  assert.deepEqual(
    storedTestPlan(cache, plan),
    plan,
    'an interrupted temporary write does not corrupt the durable plan',
  );
});

test('HS2-RRNZYN changed tree, diff, strategy or ticket requires a new reviewed plan', () => {
  for (const change of [
    { tree: 'tree-b' },
    { base: 'base-b' },
    { changes: ['b'] },
    { profile: { ...profile, ticket: 'other:native' } },
    { profile: { ...profile, developmentStrategy: { preset: 'steady-main', source: 'project' } } },
  ])
    assert.throws(() => validateTestPlan(manifest(), { ...state, ...change }), /HS2-RRNZYN/);
});

test('HS2-RRNZYN mandatory checks and deferred CI coverage cannot be waived by a plan', () => {
  const missing = manifest();
  missing.selected = [];
  assert.throws(() => verifyTestPlanSelection(missing, prepared), /selected checks changed/);
  const hidden = manifest();
  hidden.deferred = [];
  assert.throws(() => verifyTestPlanSelection(hidden, prepared), /deferred checks changed/);
  assert.throws(
    () =>
      verifyTestPlanSelection(manifest(), {
        ...prepared,
        stages: [...prepared.stages, { cwd: 'migrator', command: 'npm run test:coverage', selected: false }],
      }),
    /deferred checks changed/,
    'unaffected registered stages also require CI disclosure',
  );
  const noCoverage = manifest();
  delete noCoverage.deferred[0].ci_coverage;
  assert.throws(() => validateTestPlan(noCoverage, state), /CI coverage/);
  const badOverride = { ...manifest(), override: { reason: 'faster' } };
  assert.throws(() => validateTestPlan(badOverride, state), /author and justification/);
  const unknown = manifest();
  unknown.selected[0] = { ...unknown.selected[0], estimate_ms: null, estimate_source: 'unknown' };
  validateTestPlan(unknown, state);
  assert.deepEqual(planEstimate(unknown), { known_ms: 0, unknown_checks: 1 });
});
