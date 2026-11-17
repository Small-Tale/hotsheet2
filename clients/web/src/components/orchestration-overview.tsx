import { Button } from '@kerfjs/ui/button';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { History } from 'lucide';

import { ORCHESTRATION_ACTIONS } from '../interaction-attrs/orchestration';
import { contentionRows, type ResourceContentionView } from '../orchestration-resources';
import { decisionText, lastPassText, latestDecisions, type SchedulerDecisionPage } from '../orchestration-scheduler';
import type { OrchestrationTimeline } from '../orchestration-timeline';
import type { ClockFormat } from '../orchestration-timeline-board';
import { OrchestrationInspectorSection } from './orchestration-inspector-section';

/** What the overview read for the project; each part is absent until read. */
export interface OrchestrationOverviewData {
  /** The newest page of the scheduler's decision log. */
  decisions?: SchedulerDecisionPage;
  /** HS2-5GRXFN: `scheduler plan`'s contention: each resource that deferred work, and who waits. */
  contention?: ResourceContentionView[];
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
