import { Button } from '@kerfjs/ui/button';
import { rem } from '@kerfjs/ui/css-values';
import { FramedList, FramedListRow } from '@kerfjs/ui/framed-list';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Switch } from '@kerfjs/ui/switch';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';
import { Plus, Trash2 } from 'lucide';

import { ORCHESTRATION_SETTINGS_ACTIONS as ACTIONS } from '../interaction-attrs/orchestration-settings';
import type { PolicyDraft } from '../orchestration-settings-model';

export interface OrchestrationPolicySettingsProps {
  /** Absent while the policy is read for the first time. */
  draft?: PolicyDraft;
  saving?: boolean;
  /** The tool's display name, such as Claude for `claude`. */
  toolLabel?: (tool: string) => string;
}

/**
 * The policy part of Settings → Orchestration (docs/26 §26.8), limits only (HS2-XHQ1EH): automatic
 * placement, machine defaults, tool caps with their accounts and daily budgets (HS2-7WF6JD), wind-down
 * and build-ahead. What the orchestrator may do in a
 * project is not listed here; it asks through the permission prompt. Controls carry their draft field
 * in `name`; the delegated `change` handler updates the draft and Apply Policy stores it.
 */
export function OrchestrationPolicySettings({
  draft,
  saving = false,
  toolLabel = (tool) => tool,
}: OrchestrationPolicySettingsProps) {
  const disabled = saving || !draft;
  return (
    <section aria-label="Policy" data-component="orchestration-policy-settings">
      <List gap="l">
        <List gap="xs">
          <Text variant="h2" size="large" flush>
            Policy
          </Text>
          <Text tone="quiet" flush>
            The orchestrator starts, refills and winds down AI sessions inside these limits. The policy is checked in
            with the project.
          </Text>
        </List>
        <Switch
          name="automatic"
          label="Place sessions automatically"
          hint="Off: the orchestrator plans where sessions would run and starts none."
          checked={draft?.automatic ?? false}
          disabled={disabled}
        />
        <List gap="m">
          <Text variant="h3" size="medium" flush>
            Machine Defaults
          </Text>
          <Grid minColumnWidth={rem(14)} gap="m">
            <TextField
              name="machine.max_sessions"
              label="Sessions per Machine"
              type="number"
              inputMode="numeric"
              value={draft?.machine.max_sessions ?? ''}
              disabled={disabled}
            />
            <TextField
              name="machine.min_free_ram_mb"
              label="Free Memory Floor (MB)"
              type="number"
              inputMode="numeric"
              value={draft?.machine.min_free_ram_mb ?? ''}
              disabled={disabled}
            />
            <TextField
              name="machine.max_cpu_percent"
              label="CPU Ceiling (%)"
              type="number"
              inputMode="numeric"
              value={draft?.machine.max_cpu_percent ?? ''}
              disabled={disabled}
            />
            <TextField
              name="machine.min_free_disk_mb"
              label="Free Disk Floor (MB)"
              type="number"
              inputMode="numeric"
              value={draft?.machine.min_free_disk_mb ?? ''}
              disabled={disabled}
            />
          </Grid>
        </List>
        <List gap="m">
          <List gap="xs">
            <Text variant="h3" size="medium" flush>
              Tools
            </Text>
            <Text tone="quiet" size="compact" flush>
              Empty is Auto: the orchestrator shares each computer's sessions evenly among the tools installed and
              signed in there, within its limits above. 0 turns a tool off. With accounts, sessions run only under one
              of them, within its sessions and daily budget.
            </Text>
          </List>
          {draft && draft.tools.length > 0 ? (
            <FramedList label="Tools">
              {draft.tools.map((tool) => (
                <FramedListRow>
                  <List gap="xs">
                    <Text weight="strong">{toolLabel(tool.tool)}</Text>
                    {/* Fixed two-column grids: WebKit sizes a row too short around a wrapping or
                        auto-fill grid after a resize (KF-D8VK4Y). */}
                    <Grid columns={2} gap="m">
                      <TextField
                        name={`tool.${tool.tool}.max_sessions`}
                        label="Sessions"
                        type="number"
                        inputMode="numeric"
                        value={tool.max_sessions}
                        placeholder="Auto"
                        min={0}
                        max={64}
                        size="compact"
                        disabled={disabled}
                      />
                      <TextField
                        name={`tool.${tool.tool}.weight`}
                        label="Weight"
                        type="number"
                        inputMode="numeric"
                        value={tool.weight}
                        placeholder="Auto"
                        min={0}
                        max={100}
                        size="compact"
                        disabled={disabled}
                      />
                    </Grid>
                    {tool.accounts.map((account, index) => (
                      <Grid
                        columns={2}
                        gap="m"
                        rootAttributes={{ 'data-policy-account': `${tool.tool}:${String(index)}` }}
                      >
                        <TextField
                          name={`account.${tool.tool}.${String(index)}.account`}
                          label="Account"
                          value={account.account}
                          size="compact"
                          disabled={disabled}
                        />
                        <TextField
                          name={`account.${tool.tool}.${String(index)}.max_sessions`}
                          label="Sessions"
                          type="number"
                          inputMode="numeric"
                          value={account.max_sessions}
                          size="compact"
                          disabled={disabled}
                        />
                        <TextField
                          name={`account.${tool.tool}.${String(index)}.budget`}
                          label="Daily Budget ($)"
                          inputMode="decimal"
                          value={account.budget}
                          size="compact"
                          disabled={disabled}
                        />
                        <Row gap="xs" vAlign="bottom">
                          <Button
                            emphasis="plain"
                            size="compact"
                            ariaLabel={
                              account.account
                                ? `Remove ${account.account} from ${toolLabel(tool.tool)}`
                                : `Remove This ${toolLabel(tool.tool)} Account`
                            }
                            icon={<LucideIcon icon={Trash2} name="trash-2" />}
                            action={ACTIONS.removePolicyAccount.value}
                            disabled={disabled}
                            rootAttributes={{ 'data-tool': tool.tool, 'data-index': String(index) }}
                          >
                            Remove
                          </Button>
                        </Row>
                      </Grid>
                    ))}
                    <Row gap="xs">
                      <Button
                        emphasis="plain"
                        size="compact"
                        icon={<LucideIcon icon={Plus} name="plus" />}
                        action={ACTIONS.addPolicyAccount.value}
                        disabled={disabled}
                        rootAttributes={{ 'data-tool': tool.tool }}
                      >
                        Add Account
                      </Button>
                    </Row>
                  </List>
                </FramedListRow>
              ))}
            </FramedList>
          ) : (
            <Text tone="quiet" size="compact" flush>
              {draft ? 'No AI tools are set up on this computer.' : 'Loading tools…'}
            </Text>
          )}
        </List>
        <List gap="m">
          <Text variant="h3" size="medium" flush>
            Wind-Down and Build-Ahead
          </Text>
          <Grid minColumnWidth={rem(14)} gap="m">
            <TextField
              name="idle_minutes"
              label="Wind Down After Idle (Minutes)"
              type="number"
              inputMode="numeric"
              value={draft?.idle_minutes ?? ''}
              disabled={disabled}
            />
            <TextField
              name="keep_warm"
              label="Idle Sessions Kept per Project"
              type="number"
              inputMode="numeric"
              value={draft?.keep_warm ?? ''}
              disabled={disabled}
            />
          </Grid>
          <Switch
            name="build_ahead"
            label="Build ahead on a prerequisite whose CI is pending"
            hint="Off: the orchestrator asks on the goal ticket before building ahead. Tickets tagged speculate:never are never built ahead."
            checked={draft?.build_ahead ?? false}
            disabled={disabled}
          />
          <Switch
            name="redo_reopened"
            label="Redo reopened work on its own"
            hint="On: when built-ahead work is reopened while it runs, the orchestrator cancels that run, with the same permission as starting a session, and starts the step again once its prerequisites are confirmed. Off: it asks on the goal ticket."
            checked={draft?.redo_reopened ?? false}
            disabled={disabled}
          />
        </List>
        <Row gap="xs">
          <Button emphasis="primary" action={ACTIONS.applyPolicy.value} disabled={disabled} loading={saving}>
            Apply Policy
          </Button>
        </Row>
      </List>
    </section>
  );
}
