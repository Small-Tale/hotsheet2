import { Button } from '@kerfjs/ui/button';
import { CheckboxGroup } from '@kerfjs/ui/checkbox-group';
import { rem } from '@kerfjs/ui/css-values';
import { FramedList, FramedListRow } from '@kerfjs/ui/framed-list';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Switch } from '@kerfjs/ui/switch';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';
import {
  ArchiveRestore,
  BookOpen,
  Bot,
  Columns3,
  Database,
  GitBranch,
  ShieldCheck,
  TerminalSquare,
  Workflow,
} from 'lucide';

import type { SidebarPanelParts } from '../../components/sidebar-panel';

/**
 * The Settings rail of the mock: the project categories in their production order with the
 * proposed Orchestration category after Development strategy. The production navigator's category
 * list is closed, so the mock lists the same rows rather than changing the shipped navigator.
 */
export function orchestrationMockSettingsRail(): SidebarPanelParts {
  const rows: Array<[string, typeof Database, string, boolean]> = [
    ['Ticket sources', Database, 'database', false],
    ['AI tools', Bot, 'bot', false],
    ['Project guidance', BookOpen, 'book-open', false],
    ['Commands', TerminalSquare, 'terminal-square', false],
    ['Lifecycle', ArchiveRestore, 'archive-restore', false],
    ['Terminals', TerminalSquare, 'terminal-square', false],
    ['Permissions', ShieldCheck, 'shield-check', false],
    ['Column view', Columns3, 'columns-3', false],
    ['Development strategy', GitBranch, 'git-branch', false],
    ['Orchestration', Workflow, 'workflow', true],
  ];
  return {
    label: 'Settings categories',
    toolbar: { label: 'Settings sidebar toolbar', dividerSides: '' },
    toggle: { action: 'toggle-project-sidebar', name: 'Settings Sidebar' },
    content: (
      <List gap="m" controlInsets="tb">
        <section>
          <ListHeader label="Project Settings" />
          <nav aria-label="Project Settings">
            <List gap={rem(0.125)}>
              {rows.map(([label, icon, iconName, selected]) => (
                <ListItem
                  action="orch-mock-settings-category"
                  selected={selected}
                  icon={<LucideIcon icon={icon} name={iconName} />}
                  label={label}
                />
              ))}
            </List>
          </nav>
        </section>
      </List>
    ),
    pane: { contentElement: 'nav', contentLabel: 'Settings categories' },
  };
}

/**
 * Settings → Orchestration (§26.8): the checked-in throughput policy edited with the settings form
 * grammar of the Development strategy category. Applying is a person's act at a known revision;
 * validation lists every problem at once.
 */
export function OrchestrationMockPolicy({ invalid }: { invalid: boolean }) {
  return (
    <section aria-label="Orchestration Settings" data-orch-mock="policy">
      <List contentWidth="wide" inset="document">
        <List gap="l">
          <Text tone="quiet">
            The orchestrator starts, refills and winds down AI sessions inside these limits and asks a person for
            anything outside them. This policy is checked in with the project.
          </Text>
          {invalid && (
            <StateBanner
              copyLayout="stacked"
              tone="danger"
              title="The Policy Has 2 Problems"
              detail="Target B allows 3 sessions but its machine allows 2. Claude is listed for Target C but not configured under Tools."
            />
          )}
          <Switch
            name="orch-placement-automatic"
            label="Place sessions automatically"
            hint="Off: the scheduler plans and logs a dry run, and starts nothing."
            checked={false}
          />
          <List gap="m">
            <Text variant="h3">Machine defaults</Text>
            <Grid minColumnWidth={rem(14)} gap="m">
              <TextField
                name="orch-max-sessions"
                label="Sessions per Machine"
                type="number"
                min={1}
                max={64}
                value="2"
              />
              <TextField name="orch-min-ram" label="Free Memory Floor (MB)" type="number" min={0} value="2048" />
              <TextField name="orch-max-cpu" label="CPU Ceiling (%)" type="number" min={1} max={100} value="90" />
              <TextField name="orch-min-disk" label="Free Disk Floor (MB)" type="number" min={0} value="5120" />
            </Grid>
          </List>
          <List gap="m">
            <Text variant="h3">Tools</Text>
            <FramedList label="Tools">
              <FramedListRow>
                <Row gap="m" wrap vAlign="bottom">
                  <Text weight="strong">Codex</Text>
                  <TextField name="orch-codex-max" label="Sessions" type="number" value="3" size="compact" />
                  <TextField name="orch-codex-weight" label="Weight" type="number" value="60" size="compact" />
                  <TextField
                    name="orch-codex-budget"
                    label="Daily Budget (Cents)"
                    type="number"
                    value="2500"
                    size="compact"
                  />
                </Row>
              </FramedListRow>
              <FramedListRow>
                <Row gap="m" wrap vAlign="bottom">
                  <Text weight="strong">Claude</Text>
                  <TextField name="orch-claude-max" label="Sessions" type="number" value="2" size="compact" />
                  <TextField name="orch-claude-weight" label="Weight" type="number" value="50" size="compact" />
                  <TextField
                    name="orch-claude-budget"
                    label="Daily Budget (Cents)"
                    type="number"
                    value=""
                    size="compact"
                  />
                </Row>
              </FramedListRow>
            </FramedList>
          </List>
          <List gap="m">
            <Text variant="h3">Targets</Text>
            <FramedList label="Targets">
              {(['Target A', 'Target B', 'Target C'] as const).map((target) => (
                <FramedListRow>
                  <Row gap="m" wrap vAlign="bottom">
                    <Switch name={`orch-target-${target}`} label={target} checked />
                    <TextField
                      name={`orch-target-max-${target}`}
                      label="Sessions"
                      type="number"
                      value={target === 'Target B' && invalid ? '3' : '1'}
                      error={target === 'Target B' && invalid ? 'Above the machine limit of 2' : undefined}
                      size="compact"
                    />
                    <Select
                      name={`orch-target-tools-${target}`}
                      label="Tools"
                      value="codex-claude"
                      choices={[
                        { value: 'codex-claude', label: 'Codex, Claude' },
                        { value: 'codex', label: 'Codex' },
                        { value: 'claude', label: 'Claude' },
                      ]}
                    />
                  </Row>
                </FramedListRow>
              ))}
            </FramedList>
          </List>
          <List gap="m">
            <Text variant="h3">Approvals and wind-down</Text>
            <CheckboxGroup
              name="orch-approval"
              label="Ask a Person Before"
              orientation="horizontal"
              value={['session_start', 'deployment']}
              choices={[
                { value: 'session_start', label: 'Starting a session' },
                { value: 'session_stop', label: 'Stopping a session' },
                { value: 'work_start', label: 'Starting work' },
                { value: 'validation_run', label: 'Running validation' },
                { value: 'provisional_dispatch', label: 'Provisional work' },
                { value: 'deployment', label: 'Deploying' },
              ]}
            />
            <Grid minColumnWidth={rem(14)} gap="m">
              <TextField name="orch-idle" label="Wind Down After Idle (Minutes)" type="number" value="15" />
              <TextField name="orch-keep-warm" label="Idle Sessions Kept per Target" type="number" value="0" />
            </Grid>
            <Switch
              name="orch-speculation"
              label="Build ahead on a prerequisite whose CI is pending"
              hint="Off by default. Tickets tagged speculate:never are never built ahead."
              checked={false}
            />
          </List>
          <Row gap="xs">
            <Button emphasis="primary" action="orch-mock-apply-policy" disabled={invalid}>
              Apply Policy
            </Button>
          </Row>
        </List>
      </List>
    </section>
  );
}
