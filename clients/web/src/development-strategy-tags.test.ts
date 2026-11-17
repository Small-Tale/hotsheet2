import { describe, expect, it } from 'vitest';

import type { DevelopmentStrategyReport } from './api';
import { developmentStrategyTagSuggestions } from './development-strategy-tags';

const report = (custom: DevelopmentStrategyReport['policy']['custom']): DevelopmentStrategyReport => ({
  policy: { version: 2, default: 'steady-main', green_main_goal_minutes: 360, custom },
  effective: {
    preset: 'steady-main',
    source: 'project',
    integration: 'ci-staged',
    local_gate: 'targeted',
    require_green_main_sha: true,
    attribute_red_ci_before_completion: true,
    green_main_goal_minutes: 360,
  },
  presets: ['ticket-assured', 'steady-main', 'risk-guided'],
});

describe('development strategy tag suggestions (HS2-PN74AH)', () => {
  it('offers every built-in strategy before the policy is read, spelled as the gate reads it', () => {
    expect(developmentStrategyTagSuggestions()).toEqual([
      'development-strategy:ticket-assured',
      'development-strategy:steady-main',
      'development-strategy:risk-guided',
    ]);
  });

  it("adds the project's own custom strategies and nothing else", () => {
    const strict = {
      integration: 'serial',
      local_gate: 'full-local',
      minimum_tier: 'T1',
      browser: true,
      platform: false,
      perf: false,
      test_plan_timing: 'before-work',
      ci_gate: 'CI',
      red_ci_recovery: 'fallback-to-serial',
    } as const;
    expect(developmentStrategyTagSuggestions(report({ strict, 'night-batch': strict }))).toEqual([
      'development-strategy:ticket-assured',
      'development-strategy:steady-main',
      'development-strategy:risk-guided',
      'development-strategy:strict',
      'development-strategy:night-batch',
    ]);
    // A removed custom entry (sparse map value) is not offered, and a policy without customs adds none.
    expect(developmentStrategyTagSuggestions(report({ strict: undefined }))).toHaveLength(3);
    expect(developmentStrategyTagSuggestions(report({}))).toHaveLength(3);
  });
});
