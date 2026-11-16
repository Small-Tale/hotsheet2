import { Badge } from '@kerfjs/ui/badge';
import { Button } from '@kerfjs/ui/button';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu } from '@kerfjs/ui/popup-menu';
import { Row } from '@kerfjs/ui/row';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import {
  Ban,
  CircleCheck,
  CircleX,
  ExternalLink,
  Hand,
  Pause,
  PencilLine,
  Play,
  ShieldQuestion,
  UserRound,
} from 'lucide';

import { ORCHESTRATION_ACTIONS } from '../interaction-attrs/orchestration';
import { reconciliationRows, type RunReconciliation } from '../orchestration-run-checks';
import type { OrchestrationTimeline, TimelineItem } from '../orchestration-timeline';
import {
  cardTime,
  type ClockFormat,
  clockTime,
  itemSlug,
  machineLabel,
  stateBadge,
  toolLabel,
} from '../orchestration-timeline-board';
import { OrchestrationInspectorSection } from './orchestration-inspector-section';
import { OrchestrationPlanSection } from './orchestration-plan-section';
import { inspectorToggle, type SidebarPanelParts } from './sidebar-panel';

/** What the inspector read about the selected item's run: its gates, validation steps, and events. */
export interface OrchestrationInspectorDetails {
  loading: boolean;
  error?: string;
  ci?: string;
  gates?: ReadonlyArray<{ kind: string; state: string; provider?: string; commit?: string }>;
  validations?: ReadonlyArray<{ step_id: string; state: string; reason?: string }>;
  events?: ReadonlyArray<{ at_ms: number; kind: string; state?: string; reason?: string; source?: string }>;
  /** HS2-XM692S: the run record was read; its last reconciliation, absent when none is stored. */
  runRead?: boolean;
  reconciliation?: RunReconciliation;
}

export interface OrchestrationInspectorProps {
  item?: TimelineItem;
  timeline?: OrchestrationTimeline;
  details?: OrchestrationInspectorDetails;
  /** A run control is on its way to the server. */
  busy?: boolean;
  clock?: ClockFormat;
}

const words = (code: string) => code.replaceAll('_', ' ');
/** What the orchestrator asked to do, as a verb phrase (HS2-DBQSWB): a timeline asks only these two. */
const ASKED: Readonly<Record<string, string>> = { session_start: 'start a session', work_start: 'start work' };
const sentence = (code: string) => {
  const text = words(code);
  return text.charAt(0).toUpperCase() + text.slice(1);
};

function Note({ text }: { text: string }) {
  return (
    <List textInsets="rl">
      <Text tone="quiet" size="compact" flush>
        {text}
      </Text>
    </List>
  );
}

const control = (name: string) => ({ 'data-control': name });

/** Pause or Resume, Hand Off…, Correct… and Cancel Run…, each through the existing run routes. */
function RunControls({ item, busy }: { item: TimelineItem; busy: boolean }) {
  const run = item.run!;
  const corrections = ['completed', 'failed', 'cancelled', 'dispatched', 'intervened'].filter(
    (state) => state !== run.state,
  );
  const correctionIcon = (state: string) =>
    state === 'completed' ? CircleCheck : state === 'failed' ? CircleX : state === 'cancelled' ? Ban : UserRound;
  return (
    <Row gap="xs" wrap controlInsets="rl">
      {run.paused ? (
        <Button
          size="compact"
          action={ORCHESTRATION_ACTIONS.orchestrationRunControl.value}
          disabled={busy}
          icon={<LucideIcon size="s" icon={Play} name="play" />}
          rootAttributes={control('resume')}
        >
          Resume
        </Button>
      ) : (
        <Button
          size="compact"
          action={ORCHESTRATION_ACTIONS.orchestrationRunControl.value}
          disabled={busy}
          icon={<LucideIcon size="s" icon={Pause} name="pause" />}
          rootAttributes={control('pause')}
        >
          Pause
        </Button>
      )}
      <Button
        size="compact"
        action={ORCHESTRATION_ACTIONS.orchestrationRunControl.value}
        disabled={busy}
        icon={<LucideIcon size="s" icon={Hand} name="hand" />}
        rootAttributes={control('handoff')}
      >
        Hand Off…
      </Button>
      <PopupMenu
        text="Correct…"
        size="compact"
        emphasis="secondary"
        disabled={busy}
        icon={<LucideIcon size="s" icon={PencilLine} name="pencil-line" />}
        items={corrections.map((state) => ({
          label: `Record as ${sentence(state)}…`,
          icon: <LucideIcon icon={correctionIcon(state)} name={state} />,
          action: ORCHESTRATION_ACTIONS.orchestrationRunControl.value,
          attributes: {
            'data-control': 'correct',
            'data-run-state': state,
          },
        }))}
      />
      <Button
        size="compact"
        tone="danger"
        action={ORCHESTRATION_ACTIONS.orchestrationRunControl.value}
        disabled={busy}
        icon={<LucideIcon size="s" icon={Ban} name="ban" />}
        rootAttributes={control('cancel')}
      >
        Cancel Run…
      </Button>
    </Row>
  );
}

function WhereItRuns({
  item,
  timeline,
  clock,
}: {
  item: TimelineItem;
  timeline: OrchestrationTimeline;
  clock: ClockFormat;
}) {
  const working = item.state === 'running' || item.state === 'provisional';
  return (
    <OrchestrationInspectorSection label="Where It Runs">
      <ValueTable label="Where it runs" density="compact" stackBelow="narrow">
        <ValueTableRow
          label="Project"
          value={`${item.project_label} (${item.location === 'unbound' ? 'not bound here' : item.location})`}
        />
        <ValueTableRow label="Machine" value={machineLabel(item)} />
        <ValueTableRow label="Tool" value={toolLabel(item.tool) ?? 'Chosen when the work is placed'} />
        <ValueTableRow
          label="Session"
          value={
            item.session_id
              ? `${toolLabel(item.tool) ?? 'AI'} session, ${item.session_state ?? 'state unknown'}`
              : item.state === 'done'
                ? 'Ended'
                : 'Starts when the work is placed'
          }
        />
        <ValueTableRow
          label={working || item.state === 'done' ? 'Started' : 'Estimated start'}
          value={
            working || item.state === 'done'
              ? item.started_at_ms === undefined
                ? 'Not recorded'
                : clock(item.started_at_ms)
              : cardTime(item, timeline, clock)
          }
        />
      </ValueTable>
    </OrchestrationInspectorSection>
  );
}

function RunSection({
  item,
  busy,
  clock,
  details,
}: {
  item: TimelineItem;
  busy: boolean;
  clock: ClockFormat;
  details?: OrchestrationInspectorDetails;
}) {
  const run = item.run!;
  return (
    <OrchestrationInspectorSection label="Run">
      <ValueTable label="Run" density="compact" stackBelow="narrow">
        <ValueTableRow label="State" value={`${sentence(run.state)}${run.paused ? ', paused' : ''}`} />
        <ValueTableRow
          label="Ticket"
          value={`${itemSlug(item)}: ${item.status ? words(item.status) : 'not read yet'}`}
        />
        {run.worker_id ? <ValueTableRow label="Worker" value={run.worker_id} /> : undefined}
        <ValueTableRow
          label="Lease"
          value={run.lease_expires_at_ms === undefined ? 'None held' : `Until ${clock(run.lease_expires_at_ms)}`}
        />
        <ValueTableRow
          label="Last confirmed"
          value={
            run.unreachable_since_ms !== undefined
              ? `Unreachable since ${clock(run.unreachable_since_ms)}`
              : run.last_confirmed_at_ms === undefined
                ? 'Not confirmed yet'
                : clock(run.last_confirmed_at_ms)
          }
        />
        {run.cancel_requested ? (
          <ValueTableRow label="Cancel" value="Asked; the project has not confirmed it yet" />
        ) : undefined}
        {details?.runRead
          ? reconciliationRows(details.reconciliation, clock).map((row) => (
              <ValueTableRow label={row.label} value={row.value} />
            ))
          : undefined}
      </ValueTable>
      {(item.state === 'running' || item.state === 'provisional') && <RunControls item={item} busy={busy} />}
    </OrchestrationInspectorSection>
  );
}

function stateDetail(item: TimelineItem) {
  switch (item.state) {
    case 'waits_for_permission': {
      const permission = item.permission;
      return (
        <OrchestrationInspectorSection label="Waits for Permission">
          <Note
            text={`The orchestrator asked to ${permission ? (ASKED[permission.action] ?? words(permission.action)) : 'act'} in ${item.project_label}. ${
              permission?.kind === 'permission'
                ? 'The request is in the permission popup and in Notifications.'
                : 'The request waits for a person in Notifications.'
            }`}
          />
          <Row gap="xs" wrap controlInsets="rl">
            <Button
              size="compact"
              action={ORCHESTRATION_ACTIONS.showOrchestrationRequest.value}
              icon={<LucideIcon size="s" icon={ShieldQuestion} name="shield-question" />}
              rootAttributes={{ 'data-request-id': permission?.request_id ?? '' }}
            >
              Show Request
            </Button>
          </Row>
        </OrchestrationInspectorSection>
      );
    }
    case 'waits_for_decision':
      return (
        <OrchestrationInspectorSection label="Waits for Your Decision">
          <Note
            text={`Whether this goes ahead before its prerequisite is confirmed is asked on ${
              item.goal_slug ?? 'its goal ticket'
            }.`}
          />
          <Row gap="xs" wrap controlInsets="rl">
            <Button
              size="compact"
              action={ORCHESTRATION_ACTIONS.openOrchestrationGoal.value}
              icon={<LucideIcon size="s" icon={ExternalLink} name="external-link" />}
            >
              {`Open ${item.goal_slug ?? 'Goal'}`}
            </Button>
          </Row>
        </OrchestrationInspectorSection>
      );
    case 'waits_for_prerequisite':
      return (
        <OrchestrationInspectorSection label="Prerequisites">
          {item.waits_on && item.waits_on.length > 0 ? (
            <ValueTable label="Prerequisites" density="compact" stackBelow="narrow">
              {item.waits_on.map((wait) => (
                <ValueTableRow label={wait.slug ?? wait.native_id.slice(-6)} value={sentence(wait.reason)} />
              ))}
            </ValueTable>
          ) : (
            <Note text={(item.reasons ?? []).map(sentence).join('. ') || 'Its prerequisites are not confirmed yet.'} />
          )}
        </OrchestrationInspectorSection>
      );
    case 'ready':
      return (
        <OrchestrationInspectorSection label="Why It Is Not Running">
          <Note
            text={
              item.placement?.reason
                ? `The scheduler's last pass: ${words(item.placement.reason)}.`
                : 'It is ready; it starts when a session is free for it.'
            }
          />
        </OrchestrationInspectorSection>
      );
    case 'provisional':
      return (
        <StateBanner
          copyLayout="stacked"
          tone="info"
          title="Building Ahead"
          detail="This work rests on a prerequisite whose CI is still running. If that CI fails, this work is held back from shipping."
        />
      );
    case 'running':
    case 'done':
      return undefined;
    default:
      return item.state satisfies never;
  }
}

function Evidence({ details, clock }: { details: OrchestrationInspectorDetails; clock: ClockFormat }) {
  if (details.loading && !details.gates && !details.events)
    return <Note text="Reading gates, validation and events…" />;
  return (
    <>
      {details.error && <Note text={details.error} />}
      <OrchestrationInspectorSection label="Gates">
        {details.gates && details.gates.length > 0 ? (
          <ValueTable label="Gates" density="compact" stackBelow="narrow">
            {details.gates.map((gate) => (
              <ValueTableRow
                label={`${gate.kind === 'ci' ? 'CI' : sentence(gate.kind)}${gate.commit ? ` · ${gate.commit.slice(0, 7)}` : ''}`}
                value={sentence(gate.state)}
              />
            ))}
          </ValueTable>
        ) : (
          <Note text="No gate is linked to this run yet." />
        )}
      </OrchestrationInspectorSection>
      {details.validations && details.validations.length > 0 && (
        <OrchestrationInspectorSection label="Validation">
          <ValueTable label="Validation" density="compact" stackBelow="narrow">
            {details.validations.map((step) => (
              <ValueTableRow label={step.step_id} value={sentence(step.state)} />
            ))}
          </ValueTable>
        </OrchestrationInspectorSection>
      )}
      <OrchestrationInspectorSection label="Events">
        {details.events && details.events.length > 0 ? (
          <ValueTable label="Run events" density="compact" stackBelow="narrow">
            {details.events.map((event) => (
              <ValueTableRow
                label={clock(event.at_ms)}
                value={`${sentence(event.kind)}${event.state ? `: ${words(event.state)}` : ''}${
                  event.reason ? ` (${words(event.reason)})` : ''
                }${event.source === 'target' ? ' · learned from the project' : ''}`}
              />
            ))}
          </ValueTable>
        ) : (
          <Note text="No events yet." />
        )}
      </OrchestrationInspectorSection>
    </>
  );
}

/**
 * HS2-J89JC7: the inspector of one Orchestration view card: its goal, where it runs, its run with the
 * person's controls, what it waits on with a link to the request or the goal ticket's decision, and
 * the run's gates, validation, and events.
 */
export function OrchestrationInspector({
  item,
  timeline,
  details,
  busy = false,
  clock = clockTime,
}: OrchestrationInspectorProps) {
  if (!item || !timeline)
    return (
      <List fill hAlign="center" vAlign="middle" textInsets="rl">
        <Text tone="quiet">Select a card to see where it runs and what it waits on</Text>
      </List>
    );
  const badge = stateBadge(item);
  return (
    <List gap="l" controlInsets="tb" rootAttributes={{ 'data-orchestration-inspector': item.step_id }}>
      <List gap="xs" textInsets="rl">
        <Text variant="h2" size="medium" flush>
          {item.title ?? `${itemSlug(item)} in ${item.project_label}`}
        </Text>
        <Row gap="xs" wrap>
          <Badge tone={badge.tone} size="compact">
            {badge.label}
          </Badge>
          {item.goal_slug ? (
            <Badge tone="neutral" appearance="outline" size="compact">
              {`Goal ${item.goal_slug}${item.goal_title ? ` ${item.goal_title}` : ''}`}
            </Badge>
          ) : undefined}
        </Row>
      </List>
      <WhereItRuns item={item} timeline={timeline} clock={clock} />
      {stateDetail(item)}
      <OrchestrationPlanSection item={item} timeline={timeline} />
      {item.run && <RunSection item={item} busy={busy} clock={clock} details={details} />}
      {item.run && details && <Evidence details={details} clock={clock} />}
    </List>
  );
}

/** The right rail for the Orchestration view: the selected card's inspector. */
export function orchestrationInspectorPanel(props: OrchestrationInspectorProps): SidebarPanelParts {
  const { item } = props;
  return {
    label: 'Orchestration Inspector',
    toolbar: {
      label: 'Orchestration Inspector Toolbar',
      dividerSides: '',
      center: (
        <ToolbarText text={item ? `${itemSlug(item)} in ${item.project_label}` : 'Nothing selected'} size="small" />
      ),
    },
    toggle: inspectorToggle('Orchestration Inspector'),
    content: <OrchestrationInspector {...props} />,
    pane: {},
  };
}
