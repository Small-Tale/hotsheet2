import { Button } from '@kerfjs/ui/button';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { History } from 'lucide';

import { ORCHESTRATION_ACTIONS } from '../interaction-attrs/orchestration';
import { progressRows, type ProgressSummaryView, projectRows } from '../orchestration-progress';
import { contentionRows, type ResourceContentionView } from '../orchestration-resources';
import {
  decisionText,
  lastPassText,
  latestDecisions,
  roleStandingRows,
  type RoleStandingView,
  type SchedulerDecisionPage,
} from '../orchestration-scheduler';
import type { OrchestrationTimeline } from '../orchestration-timeline';
import type { ClockFormat } from '../orchestration-timeline-board';
import { OrchestrationInspectorSection } from './orchestration-inspector-section';

/** What the overview read for the project; each part is absent until read. */
export interface OrchestrationOverviewData {
  /** The newest page of the scheduler's decision log. */
  decisions?: SchedulerDecisionPage;
  /** HS2-XJEH08: the progress and cost summary's totals and per-project figures. */
  summary?: ProgressSummaryView;
  /** HS2-5GRXFN: `scheduler plan`'s contention: each resource that deferred work, and who waits. */
  contention?: ResourceContentionView[];
  /** HS2-65AD6J: `scheduler plan`'s integrator of each project and the role sessions it does not start. */
  roles?: RoleStandingView[];
  /** A read failed; the parts that were read still show. */
  error?: string;
}

export interface OrchestrationOverviewProps {
  timeline: OrchestrationTimeline;
  overview?: OrchestrationOverviewData;
  /** The orchestration project's name, for the heading. */
  projectName?: string;
  clock: ClockFormat;
}

const placementText = (timeline: OrchestrationTimeline) =>
  timeline.automatic
    ? 'Automatic'
    : timeline.phase === 'proposed'
      ? 'Not started; the plan is a dry run'
      : timeline.phase === 'paused'
        ? 'Paused; running work continues'
        : 'Nothing is placed';

/**
 * HS2-2XJSJK answer A: with no card selected, the inspector shows the orchestration overview. HS2-77SRHC:
 * the scheduler's placement mode, sessions and last pass, and its latest three decisions with Show All
 * Decisions…, which opens the whole log in a dialog. HS2-5GRXFN: each contended resource and who waits.
 * HS2-XJEH08: progress and cost, overall and per project; unknown is never zero and partial is labelled.
 * HS2-65AD6J: each project's integrator and any coordination session the scheduler does not start, and why.
 */
export function OrchestrationOverview({ timeline, overview, projectName, clock }: OrchestrationOverviewProps) {
  const decisions = overview?.decisions,
    lastPass = decisions ? lastPassText(decisions, clock) : undefined,
    latest = decisions ? latestDecisions(decisions) : [];
  return (
    <List gap="l" controlInsets="tb" rootAttributes={{ 'data-orchestration-overview': 'true' }}>
      <List gap="xs" textInsets="rl">
        <Text variant="h2" size="medium" flush>
          {projectName ? `${projectName} Orchestration` : 'Orchestration'}
        </Text>
        <Text tone="quiet" size="compact" flush>
          Select a card to see where it runs and what it waits on.
        </Text>
        {overview?.error ? (
          <Text tone="quiet" size="compact" flush>
            {overview.error}
          </Text>
        ) : undefined}
      </List>
      <OrchestrationInspectorSection label="Progress">
        <ValueTable label="Progress" density="compact" stackBelow="narrow">
          {progressRows(timeline, overview?.summary).map((row) => (
            <ValueTableRow label={row.label} value={row.value} />
          ))}
        </ValueTable>
      </OrchestrationInspectorSection>
      {timeline.targets.length > 0 ? (
        <OrchestrationInspectorSection label="Projects">
          <ValueTable label="Projects" density="compact" stackBelow="narrow">
            {projectRows(timeline, overview?.summary).map((row) => (
              <ValueTableRow label={row.label} value={row.value} />
            ))}
          </ValueTable>
        </OrchestrationInspectorSection>
      ) : undefined}
      <OrchestrationInspectorSection label="Scheduler">
        <ValueTable label="Scheduler" density="compact" stackBelow="narrow">
          <ValueTableRow label="Placement" value={placementText(timeline)} />
          <ValueTableRow label="Sessions" value={`${timeline.sessions} running`} />
          <ValueTableRow
            label="Last pass"
            value={lastPass ?? (decisions ? 'No pass logged yet' : 'Reading the decision log…')}
          />
        </ValueTable>
      </OrchestrationInspectorSection>
      <OrchestrationInspectorSection
        label="Coordination Sessions"
        note={
          overview?.roles === undefined
            ? 'Reading whether each project’s integrator runs…'
            : overview.roles.length === 0
              ? 'No integrator runs now, and no coordination session waits to start.'
              : undefined
        }
      >
        {overview?.roles && overview.roles.length > 0 ? (
          <ValueTable label="Coordination sessions" density="compact" stackBelow="narrow">
            {roleStandingRows(overview.roles, clock, timeline).map((row) => (
              <ValueTableRow label={row.label} value={row.value} />
            ))}
          </ValueTable>
        ) : undefined}
      </OrchestrationInspectorSection>
      <OrchestrationInspectorSection
        label="Resources"
        note={
          overview?.contention === undefined
            ? 'Reading who holds the resources work needs…'
            : overview.contention.length === 0
              ? 'No work waits on a resource.'
              : undefined
        }
      >
        {overview?.contention && overview.contention.length > 0 ? (
          <ValueTable label="Resources" density="compact" stackBelow="narrow">
            {contentionRows(overview.contention, timeline, clock).map((row) => (
              <ValueTableRow label={row.label} value={row.value} />
            ))}
          </ValueTable>
        ) : undefined}
      </OrchestrationInspectorSection>
      <OrchestrationInspectorSection
        label="Latest Decisions"
        note={decisions && latest.length === 0 ? 'The scheduler has decided nothing yet.' : undefined}
      >
        {latest.length > 0 ? (
          <ValueTable label="Latest decisions" density="compact" stackBelow="narrow">
            {latest.map((decision) => (
              <ValueTableRow label={clock(decision.at_ms)} value={decisionText(decision, timeline)} />
            ))}
          </ValueTable>
        ) : undefined}
        <Row gap="xs" wrap controlInsets="rl">
          <Button
            size="compact"
            action={ORCHESTRATION_ACTIONS.showOrchestrationDecisions.value}
            disabled={!decisions || decisions.entries.length === 0}
            icon={<LucideIcon size="s" icon={History} name="history" />}
          >
            Show All Decisions…
          </Button>
        </Row>
      </OrchestrationInspectorSection>
    </List>
  );
}
