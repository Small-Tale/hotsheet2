import { Badge } from '@kerfjs/ui/badge';
import { Button } from '@kerfjs/ui/button';
import { FramedList, FramedListRow } from '@kerfjs/ui/framed-list';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { SummaryRow } from '@kerfjs/ui/summary-row';
import { Text } from '@kerfjs/ui/text';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { Server, ServerOff, Unlink } from 'lucide';

import { inspectorToggle, type SidebarPanelParts } from '../../components/sidebar-panel';
import { BINDING_PRESENTATION, type MockScenario, type MockTarget } from './fixtures';
import { MockSection } from './orchestration-mock-step-inspector';

export const ORCH_MOCK_SELECT_TARGET = 'orch-mock-select-target';
export const ORCH_MOCK_SESSION_CONTROL = 'orch-mock-session-control';

const BINDING_ICONS: Record<MockTarget['binding'], [typeof Server, string]> = {
  reachable: [Server, 'server'],
  unreachable: [ServerOff, 'server-off'],
  unbound: [Unlink, 'unlink'],
};

/** The Targets section: one selectable row per declared target. */
export function OrchestrationMockTargets({
  targets,
  selectedTarget,
}: {
  targets: readonly MockTarget[];
  selectedTarget: string;
}) {
  return (
    <List selectionMode="single" ariaLabel="Targets">
      {targets.map((target, index) => {
        const binding = BINDING_PRESENTATION[target.binding],
          [icon, iconName] = BINDING_ICONS[target.binding];
        return (
          <SummaryRow
            title={<Text variant="span">{target.label}</Text>}
            leading={<LucideIcon icon={icon} name={iconName} />}
            trailingMeta={target.sessions}
            selected={target.id === selectedTarget}
            rail={target.binding === 'unreachable' ? 'danger' : target.binding === 'unbound' ? 'warning' : undefined}
            listEdge={index === 0 ? 'start' : index === targets.length - 1 ? 'end' : undefined}
            action={ORCH_MOCK_SELECT_TARGET}
            rootAttributes={{ 'data-target-id': target.id }}
            metadata={
              <List gap="xs">
                <Row gap="xs" wrap>
                  <Badge tone={binding.tone} size="compact">
                    {binding.label}
                  </Badge>
                  {target.server ? (
                    <Badge tone="neutral" appearance="outline" size="compact">
                      {target.server}
                    </Badge>
                  ) : undefined}
                  <Badge tone="neutral" appearance="outline" size="compact">
                    {target.grants === 1 ? '1 grant' : `${target.grants} grants`}
                  </Badge>
                  <Badge tone={target.policy === 'enabled' ? 'neutral' : 'warning'} appearance="outline" size="compact">
                    {`Policy ${target.policy}`}
                  </Badge>
                </Row>
                <Text tone="quiet" size="compact" flush>
                  {target.admission.detail}
                </Text>
              </List>
            }
          />
        );
      })}
    </List>
  );
}

function SessionRow({
  title,
  detail,
  state,
  owner,
}: {
  title: string;
  detail: string;
  state: string;
  owner: 'orchestrator' | 'person';
}) {
  return (
    <FramedListRow>
      <List gap="xs">
        <Row gap="xs" wrap vAlign="middle">
          <Text weight="strong" flush>
            {title}
          </Text>
          <Badge tone="neutral" size="compact">
            {state}
          </Badge>
          <Badge tone={owner === 'person' ? 'pop' : 'neutral'} appearance="outline" size="compact">
            {owner === 'person' ? 'Taken over by you' : 'Orchestrator owned'}
          </Badge>
        </Row>
        <Text tone="quiet" size="compact" flush>
          {detail}
        </Text>
        <Row gap="xs" wrap>
          <Button size="compact" action={ORCH_MOCK_SESSION_CONTROL} rootAttributes={{ 'data-control': 'drawer' }}>
            Show in Drawer
          </Button>
          {owner === 'orchestrator' && (
            <Button size="compact" action={ORCH_MOCK_SESSION_CONTROL} rootAttributes={{ 'data-control': 'takeover' }}>
              Take Over
            </Button>
          )}
          <Button size="compact" action={ORCH_MOCK_SESSION_CONTROL} rootAttributes={{ 'data-control': 'stop' }}>
            Stop…
          </Button>
        </Row>
      </List>
    </FramedListRow>
  );
}

function TargetDetail({ target, scenario }: { target: MockTarget; scenario: MockScenario }) {
  if (target.binding === 'unbound')
    return (
      <StateBanner
        copyLayout="stacked"
        tone="warning"
        title="Target D Is Not Bound on This Computer"
        detail="Choose the checkout this target means here. A label or repository never binds it automatically."
        action={
          <Button size="compact" shape="pill">
            Bind Target…
          </Button>
        }
      />
    );
  const unreachable = target.binding === 'unreachable',
    limited = scenario === 'degraded';
  return (
    <>
      {unreachable && (
        <StateBanner
          copyLayout="stacked"
          tone="danger"
          title="Target C Is Unreachable"
          detail="Since 14:02 (connection refused). Runs keep their last confirmed state and are marked stale."
          action={
            <Button size="compact" shape="pill">
              Review Binding…
            </Button>
          }
        />
      )}
      <MockSection label="Binding">
        <ValueTable label="Binding" density="compact" stackBelow="narrow">
          <ValueTableRow label="Server" value={target.server ?? '—'} />
          <ValueTableRow label="Checkout" value={`${target.label.toLowerCase().replace(' ', '-')}-3f2a`} />
          <ValueTableRow label="Reviewed" value="By you, 12 Oct" />
          <ValueTableRow label="State" value={BINDING_PRESENTATION[target.binding].label} />
        </ValueTable>
      </MockSection>
      <MockSection label="Grants">
        <FramedList label="Grants">
          <FramedListRow>
            <List gap="xs">
              <Row gap="xs" wrap vAlign="middle">
                <Text weight="strong" flush>
                  worker-b2
                </Text>
                <Badge tone="success" size="compact">
                  Active
                </Badge>
              </Row>
              <Text tone="quiet" size="compact" flush>
                Read and write tickets, start work, observe gates, manage sessions (Codex, up to 2) · expires 22 Dec
              </Text>
              <Row gap="xs">
                <Button size="compact">Revoke…</Button>
              </Row>
            </List>
          </FramedListRow>
        </FramedList>
      </MockSection>
      <MockSection label="Policy">
        <ValueTable label="Policy answers" density="compact" stackBelow="narrow">
          <ValueTableRow label="Start work" value="Allowed" />
          <ValueTableRow label="Start a session" value="Asks a person first" />
          <ValueTableRow label="Run validation" value="Allowed" />
          <ValueTableRow label="Deploy" value="Not permitted" />
        </ValueTable>
      </MockSection>
      <MockSection label="Capacity and admission">
        <ValueTable label="Capacity" density="compact" stackBelow="narrow">
          <ValueTableRow label="Free memory" value={unreachable ? 'Unknown' : '3.1 GB of 32 GB (floor 4 GB)'} />
          <ValueTableRow label="CPU load" value={unreachable ? 'Unknown' : '41% of 16 cores (ceiling 80%)'} />
          <ValueTableRow label="Free disk" value={unreachable ? 'Unknown' : '118 GB (floor 10 GB)'} />
          <ValueTableRow label="Sampled" value={unreachable ? 'Last at 14:01' : '30 s ago'} />
          <ValueTableRow label="Admission" value={target.admission.detail} />
        </ValueTable>
      </MockSection>
      <MockSection label="AI tool capacity">
        <ValueTable label="AI tool capacity" density="compact" stackBelow="narrow">
          <ValueTableRow label="Codex · team" value="Available" />
          <ValueTableRow
            label="Claude · personal"
            value={limited ? 'Limited until 14:30 · usage limit' : 'Available'}
          />
        </ValueTable>
      </MockSection>
      <MockSection label="Held resources">
        <ValueTable label="Held resources" density="compact" stackBelow="narrow">
          <ValueTableRow label="build-cache" value="Held by TB-17 until 14:20 · TB-20 waits" />
        </ValueTable>
      </MockSection>
      <MockSection label="Sessions">
        <FramedList label="Sessions">
          <SessionRow
            title="Codex · gpt-5 · high"
            detail="Build host 2 · worker-b2 · TB-17 · mid-turn, holds a claim (not at a safe point)"
            state="Running"
            owner="orchestrator"
          />
          <SessionRow
            title="Claude · sonnet"
            detail="Build host 2 · worker-b3 · idle 4 min · at a safe point"
            state="Running"
            owner="person"
          />
        </FramedList>
      </MockSection>
    </>
  );
}

/** The right rail for a selected target. */
export function orchestrationMockTargetPanel({
  target,
  scenario,
}: {
  target: MockTarget | undefined;
  scenario: MockScenario;
}): SidebarPanelParts {
  return {
    label: 'Target Inspector',
    toolbar: {
      label: 'Target Inspector Toolbar',
      dividerSides: '',
      center: <ToolbarText text={target?.label ?? 'Nothing selected'} size="small" />,
    },
    toggle: inspectorToggle('Target Inspector'),
    content: target ? (
      <List gap="l" controlInsets="tb" rootAttributes={{ 'data-orch-mock-inspector': target.id }}>
        <List gap="xs" textInsets="rl">
          <Text variant="h2" size="medium" flush>
            {target.label}
          </Text>
          <Row gap="xs" wrap>
            <Badge tone={BINDING_PRESENTATION[target.binding].tone} size="compact">
              {BINDING_PRESENTATION[target.binding].label}
            </Badge>
          </Row>
        </List>
        <TargetDetail target={target} scenario={scenario} />
      </List>
    ) : (
      <List fill hAlign="center" vAlign="middle">
        <Text tone="quiet">Select a target to see its binding, grants and capacity</Text>
      </List>
    ),
    pane: {},
  };
}
