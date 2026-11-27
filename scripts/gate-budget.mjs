// HS2-GN06JT: stage-level budget selection over the conservative affected-gate plan.
import { stageTimingKey, timingEstimate } from './gate-timings.mjs';
import { budgetedBrowserStage, planBrowserBudget } from './gate-browser-budget.mjs';

export const stageId = (stage) => `${stage.cwd}::${stage.command}`;

// HS2-PDNJA0: route registration and its public schema need a real-server contract run.
export const contractInputsChanged = (changes) =>
  changes.some((path) => path === 'crates/hotsheet-server/src/lib.rs' || path === 'docs/contracts/v1/openapi.yaml');

/** HS2-RRNZYN: server/client state boundaries need a shipped-flow check in addition to direct suites. */
export const highImpactBrowserChange = (changes) =>
  changes.some((path) =>
    /^(?:crates\/(?:hotsheet-server|hotsheet-ticketing|hotsheet-cli|hotsheet-aitools)\/src|clients\/web\/src)\/.*(?:auth|claim|sync|shared[-_.]state|session|checkout)/i.test(
      path,
    ),
  );

export function parseBudget(value) {
  const match = /^(\d+)(ms|s|m|h)$/.exec(value ?? '');
  if (!match) throw new Error('Budget must be a positive duration such as 5m, 300s, or 1h.');
  const multiplier = { ms: 1, s: 1000, m: 60_000, h: 3_600_000 }[match[2]];
  const ms = Number(match[1]) * multiplier;
  if (!Number.isSafeInteger(ms) || ms <= 0) throw new Error('Budget must be a positive safe duration.');
  return ms;
}

function directlyAffected(stage, changes) {
  const command = stage.command;
  if (command.startsWith('cargo nextest run '))
    return changes.some((path) => /^(crates|plugins|\.agents|\.claude|clients\/web\/public)\//.test(path));
  if (stage.cwd === 'clients/web' && command === 'npm run test:unit')
    return changes.some(
      (path) => /^clients\/web\/(src|scripts|public)\//.test(path) || /^docs\/contracts\//.test(path),
    );
  if (stage.cwd === 'migrator' && command === 'npm run test:coverage')
    return changes.some((path) => /^migrator\//.test(path));
  if (stage.cwd === 'clients/web' && command.startsWith('npx playwright test '))
    return (
      changes.some((path) => /^clients\/web\/(src|tests|public)\//.test(path) || /^docs\/contracts\//.test(path)) ||
      (contractInputsChanged(changes) && command.includes('tests/contracts.spec.ts'))
    );
  return false;
}

/**
 * HS2-C8PWWV: formatters, linters, typechecks and the cheap cross-package `always` floor. They run
 * before every suite, in their scripts/check-full.sh order, and the CI First clock never defers
 * them: a failing or slow `cargo nextest` once stopped the gate before web ESLint started, and an
 * unused import reached main.
 */
export const staticCheckStage = (stage) =>
  stage.group === 'always' ||
  stage.group === 'web_lint' ||
  /(?:^| )lint$|typecheck|format:sources:check|^cargo fmt /.test(stage.command) ||
  stage.command === 'node scripts/windows-cfg-check.mjs';

/** HS2-C8PWWV: static checks first, then suites; a stable partition keeps each side's gate order. */
export const staticChecksFirst = (stages) => [
  ...stages.filter((stage) => staticCheckStage(stage)),
  ...stages.filter((stage) => !staticCheckStage(stage)),
];

function rank(stage) {
  if (/test:unit|test:coverage|cargo nextest/.test(stage.command)) return 1;
  if (stage.command.startsWith('npx playwright test ')) return 2;
  return 3;
}

/**
 * Preserve the required floor; spend remaining known time on affected stages in relevance order.
 *
 * HS2-ZGS7JP: `bestEffort` (CI First) budgets every tier, T2 included, the same way. Analyzed
 * stages are then additive requests, not a required selection, and what does not fit is deferred
 * to CI. The gate also stops starting stages once the budget has elapsed.
 *
 * HS2-FXY4M2: under `bestEffort`, an unforced Playwright stage that `browserStages` described is
 * not an uncapped floor. Its changed-spec units are mandatory and run first; its other files and
 * grep alternatives fill what the budget leaves after the floor in relevance order, and the rest
 * are named as deferred commands. `browserCaseCounts` maps a spec file to its declared cases.
 */
export function selectBudget({
  stages,
  changes,
  history,
  budgetMs,
  tier = 'T1',
  analyzed = [],
  proposed = [],
  full = false,
  fullReason = 'full fallback',
  bestEffort = false,
  browserCaseCounts = {},
}) {
  if (!['T0', 'T1', 'T2', 'unresolved'].includes(tier)) throw new Error(`Unknown risk tier: ${tier}`);
  if (tier === 'unresolved' && !full) throw new Error('Unresolved tier must preserve the full affected selection.');
  if (bestEffort) {
    if (!Number.isSafeInteger(budgetMs) || budgetMs <= 0) throw new Error('A best-effort gate needs a positive budget.');
    // Analyzed stages become additive requests, validated like proposals.
    proposed = [...new Set([...proposed, ...analyzed])];
    analyzed = [];
  }
  // A T2 ticket under a best-effort strategy is budgeted like T0/T1.
  const analyzedT2 = tier === 'T2' && !bestEffort;
  const budgeted = ['T0', 'T1'].includes(tier) || (bestEffort && tier === 'T2');
  if (tier !== 'T2' && analyzed.length) throw new Error('Analyzed stage selection requires T2.');
  if (analyzedT2 && budgetMs !== null) throw new Error('T2 analyzed selection has no time budget.');
  if (budgeted && !full && (!Number.isSafeInteger(budgetMs) || budgetMs <= 0))
    throw new Error('T0/T1 need a positive budget.');
  const allCandidates = stages.filter((stage) => stage.selected);
  const candidateIds = new Set(allCandidates.map(stageId));
  for (const id of analyzed) if (!candidateIds.has(id)) throw new Error(`Unknown or unaffected T2 stage: ${id}`);
  for (const id of proposed) if (!candidateIds.has(id)) throw new Error(`Unknown or unaffected proposed stage: ${id}`);
  // HS2-FXY4M2: an explicitly proposed browser stage runs whole; the rest are capped by case.
  const cappedBrowser =
    bestEffort && !full
      ? allCandidates.filter((stage) => budgetedBrowserStage(stage) && !proposed.includes(stageId(stage)))
      : [];
  const cappedIds = new Set(cappedBrowser.map(stageId));
  const candidates = allCandidates.filter((stage) => !cappedIds.has(stageId(stage)));
  const selectedIds = new Set();
  const estimated = candidates.map((stage) => {
    const timing = timingEstimate(history.stages[stageTimingKey(stage)]);
    const floor =
      full ||
      stage.force === true ||
      stage.group === 'always' ||
      stage.group === 'web_lint' ||
      /(?:^| )lint$|typecheck/.test(stage.command) ||
      // HS2-V3R4JR: the Windows-target clippy pass is lint too, and the only local Windows type-check.
      stage.command === 'node scripts/windows-cfg-check.mjs' ||
      directlyAffected(stage, changes);
    if (floor) selectedIds.add(stageId(stage));
    return {
      ...stage,
      estimateMs: timing.host === 'local' ? timing.ms : null,
      estimateHost: timing.host,
      floor,
      rank: rank(stage),
    };
  });
  if (
    analyzedT2 &&
    !full &&
    !analyzed.length &&
    !proposed.length &&
    estimated.some((stage) => !stage.floor && stage.command !== 'npm run test:fixtures')
  )
    throw new Error('T2 needs an explicit analyzed stage selection for affected non-floor checks.');
  const browserFixture = estimated.find((stage) => stage.command === 'npm run test:fixtures');
  const withFixture = (stage) =>
    stage.command.startsWith('npx playwright test ') && browserFixture && !selectedIds.has(stageId(browserFixture))
      ? [browserFixture, stage]
      : [stage];
  if (estimated.some((stage) => stage.floor && stage.command.startsWith('npx playwright test ')) && browserFixture)
    selectedIds.add(stageId(browserFixture));
  let floorEstimateMs = 0;
  let floorUnknown = false;
  for (const stage of estimated.filter((stage) => selectedIds.has(stageId(stage)))) {
    if (stage.estimateMs === null) floorUnknown = true;
    else floorEstimateMs += stage.estimateMs;
  }
  let plannedMs = floorEstimateMs;
  // A ticket-specific proposal is additive and validated, even when unrelated
  // mandatory stages have unknown timing. It never changes the mandatory floor.
  for (const stage of estimated.filter((stage) => proposed.includes(stageId(stage)))) {
    for (const needed of withFixture(stage).filter((item) => !selectedIds.has(stageId(item)))) {
      selectedIds.add(stageId(needed));
      plannedMs += needed.estimateMs ?? 0;
    }
  }
  let browserPlan = null;
  if (cappedBrowser.length) {
    // An unknown floor cost counts as zero here; the run-time budget cut still bounds the gate.
    const fixtureSelected = browserFixture !== undefined && selectedIds.has(stageId(browserFixture));
    browserPlan = planBrowserBudget({
      stages: cappedBrowser,
      history,
      caseCounts: browserCaseCounts,
      remainingMs: budgetMs - plannedMs,
      fixtureMs: browserFixture?.estimateMs ?? 0,
      fixtureSelected,
    });
    if (browserPlan.fixtureNeeded && browserFixture) selectedIds.add(stageId(browserFixture));
    plannedMs += browserPlan.plannedMs;
    floorEstimateMs += browserPlan.mandatoryMs;
  }
  if (full) for (const stage of estimated) selectedIds.add(stageId(stage));
  else if (analyzedT2) {
    for (const stage of estimated.filter((stage) => analyzed.includes(stageId(stage))))
      for (const needed of withFixture(stage)) selectedIds.add(stageId(needed));
  } else if (!floorUnknown) {
    for (const stage of estimated
      .filter((stage) => !selectedIds.has(stageId(stage)) && stage.command !== 'npm run test:fixtures')
      .sort((a, b) => a.rank - b.rank || stageId(a).localeCompare(stageId(b)))) {
      const needed = withFixture(stage).filter((item) => !selectedIds.has(stageId(item)));
      if (
        needed.every((item) => item.estimateMs !== null) &&
        plannedMs + needed.reduce((sum, item) => sum + item.estimateMs, 0) <= budgetMs
      ) {
        for (const item of needed) {
          selectedIds.add(stageId(item));
          plannedMs += item.estimateMs;
        }
      }
    }
  }
  const output = staticChecksFirst(stages).flatMap((stage) => {
    if (cappedIds.has(stageId(stage)))
      return stageId(stage) === stageId(cappedBrowser[0]) ? browserPlan.stages : [];
    if (!stage.selected)
      return { ...stage, estimateMs: null, estimateHost: 'unknown', floor: false, budgetReason: 'unaffected' };
    const item = estimated.find((entry) => stageId(entry) === stageId(stage));
    const selected = selectedIds.has(stageId(stage));
    return {
      ...item,
      selected,
      budgetReason:
        tier === 'unresolved'
          ? 'unresolved tier; preserve affected gate'
          : full
            ? fullReason
            : item.floor
              ? 'mandatory floor'
              : selected
                ? analyzedT2
                  ? proposed.includes(stageId(item))
                    ? 'AI-proposed additional check'
                    : 'T2 analyzed selection'
                  : proposed.includes(stageId(item))
                    ? 'AI-proposed additional check'
                    : 'relevance within budget'
                : analyzedT2
                  ? 'deferred to CI by T2 analyzed selection'
                  : item.estimateMs === null || floorUnknown
                    ? 'unknown cost deferred to CI'
                    : bestEffort
                      ? 'deferred to CI by best-effort budget'
                      : 'deferred to CI for budget',
    };
  });
  return {
    stages: output,
    selected: output.filter((stage) => stage.selected),
    deferred: output.filter((stage) => stage.budgetReason.includes('deferred to CI')),
    budgetMs,
    floorEstimateMs: floorUnknown ? null : floorEstimateMs,
    plannedEstimateMs: floorUnknown
      ? null
      : output.filter((stage) => stage.selected).reduce((sum, stage) => sum + (stage.estimateMs ?? 0), 0),
    floorOverBudget: budgeted && !full && !floorUnknown && floorEstimateMs > budgetMs,
    tier,
    bestEffort,
    browserBudget: browserPlan?.summary ?? null,
  };
}
