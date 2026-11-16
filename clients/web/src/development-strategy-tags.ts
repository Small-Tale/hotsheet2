import type { DevelopmentPreset, DevelopmentStrategyReport } from './api';

/**
 * A ticket's development strategy override is the tag `development-strategy:<name>`, which the local
 * gate reads (docs/34-development-strategies.md). Ticket tag autocomplete offers one per strategy the
 * project can use, even when no ticket carries it yet (HS2-PN74AH).
 */
export const DEVELOPMENT_STRATEGY_TAG_PREFIX = 'development-strategy:';

/** The built-in strategies every project can name, before its policy has been read. */
export const BUILT_IN_DEVELOPMENT_STRATEGIES: readonly DevelopmentPreset[] = [
  'ticket-assured',
  'steady-main',
  'risk-guided',
];

/**
 * The override tags for one project: each built-in strategy and each custom strategy its own policy
 * defines. Another project's report is never passed in, so its custom names cannot appear here.
 */
export function developmentStrategyTagSuggestions(report?: DevelopmentStrategyReport): string[] {
  const names = new Set<string>(report?.presets.length ? report.presets : BUILT_IN_DEVELOPMENT_STRATEGIES);
  for (const [name, strategy] of Object.entries(report?.policy.custom ?? {})) if (strategy) names.add(name);
  return [...names].map((name) => `${DEVELOPMENT_STRATEGY_TAG_PREFIX}${name}`);
}
