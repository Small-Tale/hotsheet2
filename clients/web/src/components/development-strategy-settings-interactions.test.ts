import { describe, expect, it } from 'vitest';

import type { DevelopmentStrategyReport } from '../api';
import {
  changeDevelopmentStrategyField,
  draftFromDevelopmentReport,
  NEW_CUSTOM,
  policyFromDevelopmentDraft,
  selectDevelopmentStrategy,
} from './development-strategy-settings-interactions';

const report: DevelopmentStrategyReport = {
  policy: { version: 2, default: 'ticket-assured', green_main_goal_minutes: 360, custom: {} },
  effective: {
    preset: 'ticket-assured',
    source: 'project',
    integration: 'serial',
    local_gate: 'full-local',
    require_green_main_sha: true,
    attribute_red_ci_before_completion: true,
    green_main_goal_minutes: null,
  },
  presets: ['ticket-assured', 'steady-main', 'risk-guided'],
};

describe('HS2-NBDXPW development strategy draft', () => {
  it('selects a built-in without deleting saved named policies', () => {
    const draft = selectDevelopmentStrategy(draftFromDevelopmentReport(report), 'steady-main', report.policy);
    expect(policyFromDevelopmentDraft(report.policy, draft)).toEqual({
      ...report.policy,
      version: 2,
      default: 'steady-main',
      green_main_goal_minutes: 360,
      custom: {},
    });
  });

  it('keeps custom edits local, validates numeric shape, and sends required fields to the shared server', () => {
    let draft = selectDevelopmentStrategy(draftFromDevelopmentReport(report), NEW_CUSTOM, report.policy);
    draft = changeDevelopmentStrategyField(draft, 'name', 'reviewed-fast');
    draft = changeDevelopmentStrategyField(draft, 'integration', 'ci-staged');
    draft = changeDevelopmentStrategyField(draft, 'staging_branch', 'integration');
    draft = changeDevelopmentStrategyField(draft, 'red_ci_recovery', 'fallback-to-serial');
    draft = changeDevelopmentStrategyField(draft, 'browser', true);
    draft = changeDevelopmentStrategyField(draft, 'budget_minutes', '5');
    const saved = policyFromDevelopmentDraft(report.policy, draft);
    expect(saved.default_custom).toBe('reviewed-fast');
    expect(saved.custom['reviewed-fast']).toMatchObject({
      integration: 'ci-staged',
      staging_branch: 'integration',
      browser: true,
      budget_minutes: 5,
      red_ci_recovery: 'fallback-to-serial',
    });
    expect(report.policy.custom).toEqual({});
    expect(() => policyFromDevelopmentDraft(report.policy, { ...draft, greenGoal: 'NaN' })).toThrow('Green main goal');
    expect(() => policyFromDevelopmentDraft(report.policy, { ...draft, name: 'Ticket Assured' })).toThrow(
      'Custom strategy name',
    );
  });

  it('loads an existing custom profile and allows returning to a built-in', () => {
    const profile = policyFromDevelopmentDraft(report.policy, {
      ...draftFromDevelopmentReport(report),
      selection: NEW_CUSTOM,
      name: 'focused',
    });
    const customReport = { ...report, policy: profile };
    const existing = draftFromDevelopmentReport(customReport);
    expect(existing.selection).toBe('focused');
    expect(existing.custom.integration).toBe('serial');
    const builtIn = selectDevelopmentStrategy(existing, 'risk-guided', profile);
    expect(policyFromDevelopmentDraft(profile, builtIn).default_custom).toBeUndefined();
    expect(policyFromDevelopmentDraft(profile, builtIn).custom['focused']).toBeDefined();
  });
});
