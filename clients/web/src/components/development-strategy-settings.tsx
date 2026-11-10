import './development-strategy-settings.css';

import { Button } from '@kerfjs/ui/button';
import { Checkbox } from '@kerfjs/ui/checkbox';
import { rem } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { Select } from '@kerfjs/ui/select';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';

import type { DevelopmentStrategyReport } from '../api';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import type { WorkerCadence } from '../worker-view-model';
import { type DevelopmentStrategyDraft, NEW_CUSTOM } from './development-strategy-settings-interactions';

export interface DevelopmentStrategySettingsProps {
  report?: DevelopmentStrategyReport;
  draft?: DevelopmentStrategyDraft;
  loading?: boolean;
  saving?: boolean;
  message?: string;
  status?: string;
  /** One live cadence per linked Git source, from the same event stream as Workers. */
  cadence?: readonly { source: string; value: WorkerCadence }[];
}

const PRESET_COPY = {
  'ticket-assured': 'Full local verification and serial integration. Each ticket waits for a green main CI run.',
  'steady-main': 'Targeted affected verification with staged CI and a six-hour green-main goal by default.',
  'risk-guided': 'Risk-selected local checks with staged CI and exact-SHA promotion.',
} as const;

function time(seconds: number | null, empty: string): string {
  return seconds === null ? empty : new Date(seconds * 1000).toLocaleString();
}

/** Project Settings → Development strategy (HS2-NBDXPW). All writes go through the shared policy REST route. */
export function DevelopmentStrategySettings({
  report,
  draft,
  loading = false,
  saving = false,
  message = '',
  status = '',
  cadence = [],
}: DevelopmentStrategySettingsProps) {
  const selection = draft?.selection ?? report?.policy.default_custom ?? report?.policy.default ?? 'ticket-assured',
    editingCustom = selection === NEW_CUSTOM || Boolean(report?.policy.custom[selection]),
    custom = draft?.custom;
  return (
    <section class="development-strategy-settings" data-component="development-strategy-settings">
      <List gap="l">
        <List gap="xs">
          <Text tone="quiet">
            Choose how this project verifies work and moves an exact green SHA to main. A green-main deadline is a goal;
            red CI never moves main.
          </Text>
        </List>
        {loading && <Text role="status">Loading development strategy…</Text>}
        {report && draft && (
          <List gap="m">
            <Select
              name="development-selection"
              label="Project Strategy"
              value={selection}
              choices={[
                { value: 'ticket-assured', label: 'Ticket Assured' },
                { value: 'steady-main', label: 'Steady Main' },
                { value: 'risk-guided', label: 'Risk Guided' },
                ...Object.keys(report.policy.custom).map((name) => ({ value: name, label: `Custom · ${name}` })),
                { value: NEW_CUSTOM, label: 'New Custom…' },
              ]}
            />
            <Text tone="quiet" size="compact" data-development-description="true">
              {selection in PRESET_COPY
                ? PRESET_COPY[selection as keyof typeof PRESET_COPY]
                : 'Custom policy; the server validates every required gate and allowed integration strategy.'}
            </Text>
            <TextField
              name="development-greenGoal"
              label="Green Main Goal (Minutes)"
              type="number"
              min={1}
              max={10080}
              value={draft.greenGoal}
              hint="Steady Main defaults to 360 minutes (6 hours). This target never bypasses CI or promotion checks."
            />
            {editingCustom && custom && (
              <List gap="m" className="development-strategy-settings__custom">
                <Text variant="h3">Custom strategy</Text>
                {selection === NEW_CUSTOM && (
                  <TextField
                    name="development-name"
                    label="Custom Name"
                    value={draft.name}
                    hint="Use a distinct lowercase name with hyphens."
                  />
                )}
                <Grid minColumnWidth={rem(14)} gap="m">
                  <Select
                    name="development-integration"
                    label="Integration"
                    value={custom.integration}
                    choices={[
                      { value: 'serial', label: 'Serial' },
                      { value: 'batch', label: 'Batch' },
                      { value: 'ci-staged', label: 'CI Staged' },
                    ]}
                  />
                  <Select
                    name="development-local_gate"
                    label="Local Verification"
                    value={custom.local_gate}
                    choices={[
                      { value: 'full-local', label: 'Full Local' },
                      { value: 'targeted', label: 'Targeted Affected' },
                      { value: 'risk-selected', label: 'Risk Selected' },
                    ]}
                  />
                  <Select
                    name="development-minimum_tier"
                    label="Minimum Risk Tier"
                    value={custom.minimum_tier}
                    choices={[
                      { value: 'T0', label: 'T0' },
                      { value: 'T1', label: 'T1' },
                      { value: 'T2', label: 'T2' },
                    ]}
                  />
                  <TextField
                    name="development-budget_minutes"
                    label="Local Budget (Minutes, Optional)"
                    type="number"
                    min={1}
                    max={1440}
                    value={custom.budget_minutes}
                  />
                  <Select
                    name="development-test_plan_timing"
                    label="Test Plan Timing"
                    value={custom.test_plan_timing}
                    choices={[
                      { value: 'before-work', label: 'Before Work' },
                      { value: 'before-handoff', label: 'Before Handoff' },
                    ]}
                  />
                  <TextField
                    name="development-ci_gate"
                    label="Connected CI Gate"
                    value={custom.ci_gate}
                    hint="Only CI is currently supported."
                  />
                  {custom.integration === 'ci-staged' && (
                    <TextField
                      name="development-staging_branch"
                      label="Staging Branch"
                      value={custom.staging_branch}
                      hint="Must match the already allowed integration staging branch."
                    />
                  )}
                  <TextField
                    name="development-green_main_goal_minutes"
                    label="Custom Green Main Goal (Minutes, Optional)"
                    type="number"
                    min={1}
                    max={10080}
                    value={custom.green_main_goal_minutes}
                  />
                  <Select
                    name="development-red_ci_recovery"
                    label="Red CI Recovery"
                    value={custom.red_ci_recovery}
                    choices={[
                      { value: 'attribute-and-retry', label: 'Attribute and Retry' },
                      { value: 'fallback-to-serial', label: 'Fallback to Serial' },
                    ]}
                  />
                </Grid>
                <Grid minColumnWidth={rem(12)} gap="xs">
                  <Checkbox name="development-browser" label="Require Browser Checks" checked={custom.browser} />
                  <Checkbox name="development-platform" label="Require Platform Checks" checked={custom.platform} />
                  <Checkbox name="development-perf" label="Require Performance Checks" checked={custom.perf} />
                </Grid>
                <Text tone="quiet" size="compact">
                  T2 and full-local checks cannot have a time budget. Batch and CI staged require prior permission; the
                  server refuses unsupported branch, CI, and recovery combinations.
                </Text>
              </List>
            )}
            <Button action={COMMANDS_AND_AI_ACTIONS.saveDevelopmentStrategy.value} emphasis="primary" disabled={saving}>
              {saving ? 'Saving…' : 'Save Strategy'}
            </Button>
            {message ? (
              <Text tone="danger" role="alert">
                {message}
              </Text>
            ) : null}
            {status ? (
              <Text tone="quiet" role="status">
                {status}
              </Text>
            ) : null}
          </List>
        )}
        {report && (
          <List gap="xs" className="development-strategy-settings__effective">
            <Text variant="h3">Effective policy</Text>
            <Text tone="quiet" size="compact">
              Preview an explicit session or ticket selection without saving it to this project. Ticket takes
              precedence.
            </Text>
            {draft ? (
              <Grid minColumnWidth={rem(14)} gap="m">
                <TextField
                  name="development-sessionOverride"
                  label="Session Override (Optional)"
                  value={draft.sessionOverride}
                />
                <TextField
                  name="development-ticketOverride"
                  label="Ticket Override (Optional)"
                  value={draft.ticketOverride}
                />
              </Grid>
            ) : null}
            <Button
              action={COMMANDS_AND_AI_ACTIONS.previewDevelopmentStrategy.value}
              emphasis="secondary"
              disabled={loading || saving}
            >
              Preview Effective Policy
            </Button>
            <Text>{`Selection: ${report.effective.preset} · ${report.effective.source.replaceAll('-', ' ')}`}</Text>
            <Text>{`Local verification: ${report.effective.local_gate.replaceAll('-', ' ')} · integration: ${report.effective.integration}`}</Text>
            <Text>{`Connected CI: ${report.effective.custom?.ci_gate ?? 'CI'} · ${report.effective.require_green_main_sha ? 'exact green main SHA required' : 'main SHA requirement absent'}`}</Text>
            <Text>{`${report.effective.attribute_red_ci_before_completion ? 'Red CI must be attributed before completion' : 'Red CI attribution requirement absent'}${report.effective.custom ? ` · ${report.effective.custom.red_ci_recovery.replaceAll('-', ' ')}` : ''}`}</Text>
            {report.effective.green_main_goal_minutes !== null && (
              <Text>{`Green main goal: ${report.effective.green_main_goal_minutes} minutes`}</Text>
            )}
          </List>
        )}
        {cadence.map(({ source, value }) => (
          <div class="development-strategy-settings__cadence" data-cadence-source={source}>
            <List gap="xs">
              <Text variant="h3">Current cadence</Text>
              <div class="development-strategy-settings__source" title={source}>
                Source: {source.length > 16 ? `${source.slice(0, 12)}…` : source}
              </div>
              <Text>{`Last green main promotion: ${time(value.last_green_main_at, 'No recorded green promotion yet')}`}</Text>
              <Text>{`Next target: ${time(value.next_target_at, 'No target scheduled')}`}</Text>
              <Text>{`Current action: ${value.action.replaceAll('_', ' ')}`}</Text>
              {value.missed_goal_reason ? (
                <Text tone="quiet">{`Missed goal: ${value.missed_goal_reason.replaceAll('_', ' ')}`}</Text>
              ) : null}
            </List>
          </div>
        ))}
      </List>
    </section>
  );
}
