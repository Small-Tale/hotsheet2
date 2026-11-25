// HS2-FXY4M2: CI First browser selection within the local planning budget.
//
// `browserStages` (scripts/local-gate.mjs) describes each Playwright stage it builds with a
// `browser` record: the whole spec files it runs, or the `--grep` alternatives of one spec file,
// each with a relevance rank. Under a best-effort strategy the gate no longer treats those stages
// as an uncapped mandatory floor. It splits them into units (one whole file, or one grep
// alternative), estimates each unit from the timing history the gate already records, and always
// runs a changed spec's own units and the units that exercise the changed module, even past the
// budget (the maintainer's "keep cases for the changed module" decision). The exercised set is
// bounded by KEPT_EXTRA_MS. Every other unit fills only what the budget leaves after the floor, in
// rank order. Every unit that does not run is named as a runnable deferred command, so the plan,
// log and receipt show exactly what covering main CI must prove.
import { timingEstimate } from './gate-timings.mjs';

/** Relevance ranks, most relevant first. A changed spec's own tests always run. */
export const BROWSER_RANK = Object.freeze({
  changed: 0,
  regression: 1,
  exercised: 2,
  baseline: 3,
  heuristic: 4,
});

/** Per Playwright invocation: config load, the shared dev server start and browser launch. */
export const BROWSER_COMMAND_OVERHEAD_MS = 5_000;

/**
 * Safety valve for kept exercised-module cases: at most this much planned time beyond what the
 * budget leaves after the floor. A larger exercised set keeps its cheapest cases and defers the rest.
 */
export const KEPT_EXTRA_MS = 180_000;

/** Locale-independent ordering, so every host plans the same selection. */
const compare = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
const quote = (value) => `'${value.replaceAll("'", "'\\''")}'`;
const percentile = (values, rank) =>
  values.length ? [...values].sort((a, b) => a - b)[Math.ceil(values.length * rank) - 1] : null;
/** Timing history names Playwright files relative to the test directory; the gate names `tests/…`. */
const historyFile = (file) => file.replace(/^tests\//, '');

/** Statically declared Playwright cases in a spec source; loops and generated cases count once. */
export function countDeclaredTests(source) {
  return (source.match(/(?:^|[^\w.$])test(?:\.(?:only|skip|fixme|fail|slow))?\(\s*['"`]/gm) ?? []).length;
}

/** A Playwright stage the best-effort browser budget may split and cap. */
export const budgetedBrowserStage = (stage) =>
  stage.selected === true &&
  stage.force !== true &&
  stage.browser !== undefined &&
  stage.command.startsWith('npx playwright test ');

/** One whole file or one grep alternative per unit, in the stage's declared order. */
export function browserUnits(stage) {
  const browser = stage.browser;
  if (browser.alternatives)
    return browser.alternatives.map((entry) => ({
      file: browser.file,
      grep: entry.pattern,
      rank: entry.rank,
      changed: entry.rank === BROWSER_RANK.changed,
      kept: entry.rank === BROWSER_RANK.exercised,
    }));
  return browser.files.map((entry) => ({
    file: entry.file,
    grep: null,
    rank: entry.rank,
    changed: entry.rank === BROWSER_RANK.changed,
    kept: entry.rank === BROWSER_RANK.exercised,
  }));
}

/**
 * p75 duration of one passing case per spec file, from the per-case attempt ledger
 * (`attempts[].cases`, ids `playwright:<file>#<title hash>`).
 */
export function caseEstimates(history) {
  const byFile = new Map();
  for (const attempt of history.attempts ?? [])
    for (const observation of attempt.cases ?? []) {
      const match = /^playwright:(.+)#[0-9a-f]+$/.exec(observation.id ?? '');
      if (!match || observation.outcome !== 'pass' || !(observation.ms > 0)) continue;
      const file = historyFile(match[1]);
      if (!byFile.has(file)) byFile.set(file, []);
      byFile.get(file).push(observation.ms);
    }
  return new Map([...byFile].map(([file, values]) => [file, percentile(values, 0.75)]));
}

/**
 * A unit's planned cost: a whole file uses its recorded per-file estimate; a grep alternative uses
 * the recorded per-case p75 for that file, else the file estimate divided by its declared cases.
 * Returns null for a file with no recorded timing.
 */
export function unitEstimate(unit, { history, caseCounts = {}, perCase = caseEstimates(history) }) {
  const name = historyFile(unit.file);
  const records = [...(history.files?.[`playwright:${name}`] ?? []), ...(history.files?.[`playwright:tests/${name}`] ?? [])];
  const file = timingEstimate(records);
  if (unit.grep === null) return { ms: file.ms, host: file.host };
  const observed = perCase.get(name);
  if (observed) return { ms: Math.ceil(observed), host: 'local' };
  if (file.ms === null) return { ms: null, host: 'unknown' };
  return { ms: Math.ceil(file.ms / Math.max(1, caseCounts[unit.file] ?? 1)), host: file.host };
}

/**
 * Choose browser units for `remainingMs`, the budget left after the floor (negative when the floor
 * alone is over budget).
 * 1. Changed-spec units always run.
 * 2. Exercised-module units with a known cost are kept cheapest first, up to the larger of
 *    `remainingMs` and the changed-spec cost, plus `keptExtraMs`; the rest are trimmed to CI.
 * 3. Every other known-cost unit is taken first-fit in (rank, spec file, declared order) only while
 *    the whole browser plan stays within `remainingMs`.
 * Ties never depend on stage order. The first unit of each new Playwright command, and the shared
 * fixture build when nothing else needs it, add their overhead. Changed and kept units count in the
 * floor; one that ends past `remainingMs` is marked `overBudget`.
 */
export function capBrowserUnits({
  units,
  remainingMs,
  fixtureMs = 0,
  fixtureSelected = false,
  keptExtraMs = KEPT_EXTRA_MS,
}) {
  const ordered = units
    .map((unit, order) => ({ ...unit, order }))
    .sort((a, b) => a.rank - b.rank || compare(a.file, b.file) || a.order - b.order);
  const commands = new Set();
  let fixture = fixtureSelected;
  let plannedMs = 0;
  let mandatoryMs = 0;
  const cost = (unit) =>
    (unit.estimateMs ?? 0) +
    (commands.has(unit.commandKey) ? 0 : BROWSER_COMMAND_OVERHEAD_MS) +
    (fixture ? 0 : fixtureMs);
  const take = (unit) => {
    const ms = cost(unit);
    commands.add(unit.commandKey);
    fixture = true;
    plannedMs += ms;
    return ms;
  };
  const decided = new Map();
  const overBudget = new Set();
  const floorTake = (unit, decision) => {
    mandatoryMs += take(unit);
    decided.set(unit.order, decision);
    if (plannedMs > remainingMs) overBudget.add(unit.order);
  };
  for (const unit of ordered.filter((unit) => unit.changed)) floorTake(unit, 'mandatory');
  const keptLimit = Math.max(remainingMs, plannedMs) + keptExtraMs;
  const kept = ordered
    .filter((unit) => unit.kept)
    .sort(
      (a, b) =>
        (a.estimateMs ?? Infinity) - (b.estimateMs ?? Infinity) || compare(a.file, b.file) || a.order - b.order,
    );
  for (const unit of kept) {
    if (unit.estimateMs === null) decided.set(unit.order, 'unknown');
    else if (plannedMs + cost(unit) <= keptLimit) floorTake(unit, 'kept');
    else decided.set(unit.order, 'trimmed');
  }
  for (const unit of ordered.filter((unit) => !unit.changed && !unit.kept)) {
    if (unit.estimateMs === null) decided.set(unit.order, 'unknown');
    else if (plannedMs + cost(unit) <= remainingMs) {
      take(unit);
      decided.set(unit.order, 'selected');
    } else decided.set(unit.order, 'deferred');
  }
  return {
    units: ordered.map((unit) => ({ ...unit, decision: decided.get(unit.order), overBudget: overBudget.has(unit.order) })),
    plannedMs,
    mandatoryMs,
    fixtureNeeded: fixture && !fixtureSelected,
  };
}

/** A runnable Playwright command for some units of one original stage. */
export function unitsCommand(units) {
  const whole = [...new Set(units.filter((unit) => unit.grep === null).map((unit) => unit.file))].sort();
  if (whole.length) return `npx playwright test ${whole.map(quote).join(' ')}`;
  return `npx playwright test ${quote(units[0].file)} --grep ${quote(units.map((unit) => unit.grep).join('|'))}`;
}

const unitLabel = (unit) => (unit.grep === null ? unit.file : `${unit.file} --grep ${unit.grep}`);

/**
 * Split each budgeted Playwright stage by decision. A stage whose units share one decision keeps
 * its original command; a mixed stage becomes one stage per decision. Selected stages are ordered
 * changed spec first, then by best rank, so the run-time budget cut reaches them last.
 */
export function planBrowserBudget({
  stages,
  history,
  caseCounts = {},
  remainingMs,
  fixtureMs = 0,
  fixtureSelected = false,
  keptExtraMs = KEPT_EXTRA_MS,
}) {
  const perCase = caseEstimates(history);
  const units = stages.flatMap((stage, stageIndex) =>
    browserUnits(stage).map((unit) => {
      const estimate = unitEstimate(unit, { history, caseCounts, perCase });
      return {
        ...unit,
        stageIndex,
        // Changed and kept units split into their own commands, so they carry their own overhead.
        commandKey: `${stageIndex}:${unit.changed ? 'changed' : unit.kept ? 'kept' : 'other'}`,
        estimateMs: estimate.ms,
        estimateHost: estimate.host,
      };
    }),
  );
  const capped = capBrowserUnits({ units, remainingMs, fixtureMs, fixtureSelected, keptExtraMs });
  const reasons = {
    mandatory: 'HS2-FXY4M2: changed spec tests always run first',
    kept: 'HS2-FXY4M2: cases that exercise the changed module are kept, even over budget',
    selected: 'HS2-FXY4M2: browser cases within the best-effort budget',
    deferred: 'deferred to CI by best-effort browser budget',
    trimmed: 'deferred to CI by the kept-case cap',
    unknown: 'unknown cost deferred to CI',
  };
  const order = { mandatory: 0, kept: 1, selected: 2, deferred: 3, trimmed: 4, unknown: 5 };
  const runs = (decision) => decision === 'mandatory' || decision === 'kept' || decision === 'selected';
  const floor = (decision) => decision === 'mandatory' || decision === 'kept';
  const output = [];
  stages.forEach((stage, stageIndex) => {
    const own = capped.units.filter((unit) => unit.stageIndex === stageIndex).sort((a, b) => a.order - b.order);
    const groups = new Map();
    for (const unit of own) {
      if (!groups.has(unit.decision)) groups.set(unit.decision, []);
      groups.get(unit.decision).push(unit);
    }
    for (const [decision, members] of groups) {
      const estimateMs = members.some((unit) => unit.estimateMs === null)
        ? null
        : members.reduce((sum, unit) => sum + unit.estimateMs, 0) + BROWSER_COMMAND_OVERHEAD_MS;
      output.push({
        ...stage,
        command: groups.size === 1 ? stage.command : unitsCommand(members),
        selected: runs(decision),
        floor: floor(decision),
        estimateMs,
        estimateHost: members.every((unit) => unit.estimateHost === 'local') ? 'local' : members.some((unit) => unit.estimateHost === 'unknown') ? 'unknown' : 'ci',
        budgetReason: reasons[decision],
        browserRank: Math.min(...members.map((unit) => unit.rank)),
        browserCases: members.length,
        browserDecision: decision,
      });
    }
  });
  output.sort(
    (a, b) =>
      order[a.browserDecision] - order[b.browserDecision] ||
      a.browserRank - b.browserRank ||
      compare(a.command, b.command),
  );
  const count = (decision) => capped.units.filter((unit) => unit.decision === decision).length;
  const left = capped.units.filter((unit) => !runs(unit.decision));
  return {
    stages: output,
    plannedMs: capped.plannedMs,
    mandatoryMs: capped.mandatoryMs,
    fixtureNeeded: capped.fixtureNeeded,
    summary: {
      remainingMs,
      plannedMs: capped.plannedMs,
      mandatory: count('mandatory'),
      kept: count('kept'),
      selected: count('selected'),
      // The changed-spec and kept cases planned past the budget, and by how much.
      keptOverBudgetMs: Math.max(0, capped.mandatoryMs - Math.max(0, remainingMs)),
      keptOverBudgetCases: capped.units.filter((unit) => unit.overBudget).map(unitLabel),
      trimmed: count('trimmed'),
      deferred: left.length,
      deferredCases: left.map(unitLabel),
    },
  };
}
