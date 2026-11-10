import { CatalogExample } from '@kerfjs/ui/catalog';

import type { DevelopmentStrategyReport } from '../api';
import { DevelopmentStrategySettings } from '../components/development-strategy-settings';
import { draftFromDevelopmentReport } from '../components/development-strategy-settings-interactions';

/** Synthetic project policy; never reads a running checkout (HS2-NBDXPW). */
export const DEMO_DEVELOPMENT_REPORT: DevelopmentStrategyReport = {
  policy: { version: 2, default: 'steady-main', green_main_goal_minutes: 360, custom: {} },
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
};

export function DevelopmentStrategySettingsDemo() {
  return (
    <CatalogExample label="Steady Main" viewport={{ width: 'full' }}>
      <DevelopmentStrategySettings
        report={DEMO_DEVELOPMENT_REPORT}
        draft={draftFromDevelopmentReport(DEMO_DEVELOPMENT_REPORT)}
        cadence={[
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
              tickets_promoted_in_goal_window: 2,
              ready_batch: null,
              action: 'await_exact_sha_ci',
              missed_goal_reason: null,
            },
          },
        ]}
      />
    </CatalogExample>
  );
}
