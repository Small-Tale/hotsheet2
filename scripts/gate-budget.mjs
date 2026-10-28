// HS2-GN06JT: stage-level budget selection over the conservative affected-gate plan.
import { stageTimingKey, timingEstimate } from './gate-timings.mjs';

export const stageId = (stage) => `${stage.cwd}::${stage.command}`;

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
    return changes.some((path) => /^clients\/web\/(src|tests|public)\//.test(path) || /^docs\/contracts\//.test(path));
  return false;
}

function rank(stage) {
  if (/test:unit|test:coverage|cargo nextest/.test(stage.command)) return 1;
  if (stage.command.startsWith('npx playwright test ')) return 2;
  return 3;
}

/** Preserve the required floor; spend remaining known time on affected stages in relevance order. */
export function selectBudget({
  stages,
  changes,
  history,
  budgetMs,
  tier = 'T1',
  analyzed = [],
  full = false,
  fullReason = 'full fallback',
}) {
  if (!['T0', 'T1', 'T2', 'unresolved'].includes(tier)) throw new Error(`Unknown risk tier: ${tier}`);
  if (tier === 'unresolved' && !full) throw new Error('Unresolved tier must preserve the full affected selection.');
  if (tier !== 'T2' && analyzed.length) throw new Error('Analyzed stage selection requires T2.');
  if (tier === 'T2' && budgetMs !== null) throw new Error('T2 analyzed selection has no time budget.');
  if (['T0', 'T1'].includes(tier) && !full && (!Number.isSafeInteger(budgetMs) || budgetMs <= 0))
    throw new Error('T0/T1 need a positive budget.');
  const candidates = stages.filter((stage) => stage.selected);
  const candidateIds = new Set(candidates.map(stageId));
  for (const id of analyzed) if (!candidateIds.has(id)) throw new Error(`Unknown or unaffected T2 stage: ${id}`);
  const selectedIds = new Set();
  const estimated = candidates.map((stage) => {
    const timing = timingEstimate(history.stages[stageTimingKey(stage)]);
    const floor =
      full ||
      stage.force === true ||
      stage.group === 'always' ||
      /(?:^| )lint$|typecheck/.test(stage.command) ||
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
    tier === 'T2' &&
    !full &&
    !analyzed.length &&
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
  if (full) for (const stage of estimated) selectedIds.add(stageId(stage));
  else if (tier === 'T2') {
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
  const output = stages.map((stage) => {
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
                ? tier === 'T2'
                  ? 'T2 analyzed selection'
                  : 'relevance within budget'
                : tier === 'T2'
                  ? 'deferred to CI by T2 analyzed selection'
                  : item.estimateMs === null || floorUnknown
                    ? 'unknown cost deferred to CI'
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
    floorOverBudget: ['T0', 'T1'].includes(tier) && !full && !floorUnknown && floorEstimateMs > budgetMs,
    tier,
  };
}
