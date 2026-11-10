import { describe, expect, it } from 'vitest';

import type { DevelopmentStrategyReport } from '../api';
import { DevelopmentStrategySettings } from './development-strategy-settings';
import { draftFromDevelopmentReport, NEW_CUSTOM } from './development-strategy-settings-interactions';

const report: DevelopmentStrategyReport = {
  policy: { version: 2, default: 'steady-main', green_main_goal_minutes: 360, custom: {} },
  effective: {
    preset: 'steady-main',
    source: 'ticket',
    integration: 'ci-staged',
    local_gate: 'targeted',
    require_green_main_sha: true,
    attribute_red_ci_before_completion: true,
    green_main_goal_minutes: 360,
  },
  presets: ['ticket-assured', 'steady-main', 'risk-guided'],
};

describe('HS2-NBDXPW project strategy settings', () => {
  it('shows all presets, effective override provenance, goal and live source cadence', () => {
    const markup = String(
      DevelopmentStrategySettings({
        report,
        draft: draftFromDevelopmentReport(report),
        cadence: [
          {
            source: 'synthetic-git',
            value: {
              goal_minutes: 360,
              last_green_main_at: 1_700_000_000,
              next_target_at: 1_700_021_600,
              oldest_staging_at: null,
              staging_age_seconds: null,
              ci_failures_since_promotion: 0,
              correction_wall_seconds: null,
              tickets_promoted_in_goal_window: 1,
              ready_batch: null,
              action: 'await_exact_sha_ci',
              missed_goal_reason: null,
            },
          },
        ],
      }),
    );
    expect(markup).toContain('Project Strategy');
    expect(markup).toContain('Ticket Assured');
    expect(markup).toContain('Steady Main');
    expect(markup).toContain('Risk Guided');
    expect(markup).toContain('ticket');
    expect(markup).toContain('360 minutes');
    expect(markup).toContain('Current cadence');
    expect(markup).toContain('synthetic-git');
    expect(markup).toContain('red CI never moves main');
  });

  it('shows the complete Custom form and preserves a visible server refusal', () => {
    const draft = { ...draftFromDevelopmentReport(report), selection: NEW_CUSTOM, name: 'custom-one' };
    const markup = String(
      DevelopmentStrategySettings({ report, draft, message: 'CI is not configured for this checkout.' }),
    );
    for (const field of [
      'integration',
      'local_gate',
      'minimum_tier',
      'budget_minutes',
      'browser',
      'platform',
      'perf',
      'test_plan_timing',
      'ci_gate',
      'green_main_goal_minutes',
      'red_ci_recovery',
    ])
      expect(markup).toContain(`name="development-${field}"`);
    expect(markup).toContain('CI is not configured');
    expect(markup).toContain('role="alert"');
    expect(markup).toContain('save-development-strategy');
  });

  it('shows a readable source and distinguishes an unscheduled target from no promotion', () => {
    const markup = String(
      DevelopmentStrategySettings({
        report,
        cadence: [
          {
            source: 'synthetic-source-0123456789abcdef',
            value: {
              goal_minutes: 360,
              last_green_main_at: null,
              next_target_at: null,
              oldest_staging_at: null,
              staging_age_seconds: null,
              ci_failures_since_promotion: 0,
              correction_wall_seconds: null,
              tickets_promoted_in_goal_window: 0,
              ready_batch: null,
              action: 'none',
              missed_goal_reason: null,
            },
          },
        ],
      }),
    );
    expect(markup).toContain('Source: synthetic-so…');
    expect(markup).toContain('title="synthetic-source-0123456789abcdef"');
    expect(markup).toContain('Last green main promotion: No recorded green promotion yet');
    expect(markup).toContain('Next target: No target scheduled');
  });
});
