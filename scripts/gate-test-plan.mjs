// HS2-RRNZYN: reviewed, exact-tree selection for a ticket's local verification.
import { createHash, randomUUID } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const hash = (value) => createHash('sha256').update(JSON.stringify(value)).digest('hex');
const nonempty = (value) => typeof value === 'string' && value.trim().length > 0;

export function planIdentity(plan) {
  return hash(plan);
}

export function planEstimate(plan) {
  return {
    known_ms: plan.selected.reduce((sum, entry) => sum + (entry.estimate_ms ?? 0), 0),
    unknown_checks: plan.selected.filter((entry) => entry.estimate_ms === null).length,
  };
}

export function validateTestPlan(manifest, state) {
  if (!manifest || manifest.version !== 1) throw new Error('HS2-RRNZYN: test plan needs version 1.');
  if (!nonempty(manifest.ticket) || manifest.ticket !== state.profile.ticket)
    throw new Error('HS2-RRNZYN: test plan ticket does not match the resolved ticket.');
  if (manifest.tree !== state.tree || manifest.base !== state.base)
    throw new Error('HS2-RRNZYN: test plan tree/base changed; review and replan the current diff.');
  if (JSON.stringify(manifest.changed_paths) !== JSON.stringify([...state.changes].sort()))
    throw new Error('HS2-RRNZYN: test plan changed paths no longer match the reviewed diff.');
  if (
    manifest.strategy !== (state.profile.developmentStrategy?.preset ?? 'risk-selected') ||
    manifest.strategy_source !== (state.profile.developmentStrategy?.source ?? 'unconfigured-project')
  )
    throw new Error('HS2-RRNZYN: effective project strategy changed; review and replan.');
  if (
    !nonempty(manifest.behavior) ||
    !Array.isArray(manifest.risks) ||
    !manifest.risks.every(nonempty) ||
    !manifest.risks.length
  )
    throw new Error('HS2-RRNZYN: describe behavior and concrete risks in the reviewed plan.');
  if (!nonempty(manifest.reviewed_by)) throw new Error('HS2-RRNZYN: name the reviewer of the exact-tree test plan.');
  for (const key of ['analyzed', 'proposed'])
    if (
      !Array.isArray(manifest[key]) ||
      !manifest[key].every(nonempty) ||
      new Set(manifest[key]).size !== manifest[key].length
    )
      throw new Error(`HS2-RRNZYN: ${key} must contain unique stage identifiers.`);
  for (const key of ['selected', 'deferred']) {
    if (
      !Array.isArray(manifest[key]) ||
      !manifest[key].every(
        (entry) =>
          nonempty(entry.id) &&
          nonempty(entry.reason) &&
          (key === 'selected'
            ? (Number.isFinite(entry.estimate_ms) && entry.estimate_ms >= 0) ||
              (entry.estimate_ms === null && entry.estimate_source === 'unknown')
            : nonempty(entry.ci_coverage)),
      )
    )
      throw new Error(
        `HS2-RRNZYN: ${key} checks need reasons and ${key === 'selected' ? 'nonnegative or explicitly unknown estimates' : 'CI coverage'}.`,
      );
    if (new Set(manifest[key].map((entry) => entry.id)).size !== manifest[key].length)
      throw new Error(`HS2-RRNZYN: duplicate ${key} check.`);
  }
  if (manifest.override !== undefined && (!nonempty(manifest.override.reason) || !nonempty(manifest.override.author)))
    throw new Error('HS2-RRNZYN: an override needs a named author and justification.');
  return manifest;
}

export function verifyTestPlanSelection(manifest, prepared) {
  const check = (key, stages) => {
    const actual = stages.map((stage) => `${stage.cwd}::${stage.command}`).sort();
    const planned = manifest[key].map((entry) => entry.id).sort();
    if (JSON.stringify(actual) !== JSON.stringify(planned))
      throw new Error(`HS2-RRNZYN: ${key} checks changed; review and replan before running.`);
  };
  check('selected', prepared.checks);
  // `prepared.deferred` is only budget-deferred affected work. The reviewed plan
  // must also disclose unaffected registered stages that covering CI still runs.
  check(
    'deferred',
    prepared.stages.filter((stage) => !stage.selected),
  );
}

export function testPlanPath(cache, { tree, base, ticket }) {
  return join(cache, 'plans', `${hash({ tree, base, ticket })}.json`);
}

export function storedTestPlan(cache, identity) {
  const path = testPlanPath(cache, identity);
  return existsSync(path) ? JSON.parse(readFileSync(path, 'utf8')) : null;
}

export function rememberTestPlan(cache, manifest) {
  const path = testPlanPath(cache, manifest);
  mkdirSync(join(cache, 'plans'), { recursive: true });
  const temporary = `${path}.${randomUUID()}.tmp`;
  try {
    writeFileSync(temporary, JSON.stringify(manifest, null, 2) + '\n');
    renameSync(temporary, path);
  } finally {
    rmSync(temporary, { force: true });
  }
}
