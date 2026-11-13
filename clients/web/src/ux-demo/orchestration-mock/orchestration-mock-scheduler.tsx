import { Badge, type SemanticTone } from '@kerfjs/ui/badge';
import { Button } from '@kerfjs/ui/button';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { Row } from '@kerfjs/ui/row';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { SummaryRow } from '@kerfjs/ui/summary-row';
import { Text } from '@kerfjs/ui/text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';

import type { MockScenario } from './fixtures';
import { ORCH_MOCK_SELECT_STEP } from './orchestration-mock-work';

interface PlanEntry {
  step: string;
  title: string;
  decision: string;
  tone: SemanticTone;
  reason: string;
  score: string;
}

const plan = (scenario: MockScenario): PlanEntry[] => [
  {
    step: 'step-ta47',
    title: 'TA-47 Move Target A to the new toolchain',
    decision: 'Not placed',
    tone: 'warning',
    reason: 'Waiting for your approval to start Claude on Workstation 1',
    score: '5,566',
  },
  {
    step: 'step-tb20',
    title: 'TB-20 Move Target B to the new toolchain',
    decision: 'Not placed',
    tone: 'warning',
    reason: 'build-cache on Build host 2 is held by TB-17 until 14:20',
    score: '4,700',
  },
  scenario === 'degraded'
    ? {
        step: 'step-tc11',
        title: 'TC-11 Accept a retried sync',
        decision: 'Not ready',
        tone: 'danger',
        reason: 'A prerequisite gate failed (TA-44 CI)',
        score: '—',
      }
    : {
        step: 'step-tc11',
        title: 'TC-11 Accept a retried sync',
        decision: 'Start',
        tone: 'success',
        reason: 'Codex on Build host 3, account team',
        score: '3,900',
      },
  {
    step: 'step-tb17',
    title: 'TB-17 Read export schema version 2',
    decision: 'Keep',
    tone: 'neutral',
    reason: 'Session busy on its run',
    score: '—',
  },
];

interface Decision {
  time: string;
  title: string;
  detail: string;
  outcome: string;
  tone: SemanticTone;
}

const decisions: Decision[] = [
  {
    time: '14:06',
    title: 'Not placed: TA-47',
    detail: 'approval_required · session_start · 1 of 2 sessions on Workstation 1',
    outcome: 'Approval filed',
    tone: 'warning',
  },
  {
    time: '14:06',
    title: 'Not placed: TB-20',
    detail: 'resource_held · build-cache until 14:20',
    outcome: 'Deferred',
    tone: 'warning',
  },
  {
    time: '14:05',
    title: 'Start: TB-17 on Build host 2',
    detail: 'Codex team · 1 of 2 target, 1 of 2 machine · free RAM 7.4 GB, footprint 1.5 GB',
    outcome: 'Started',
    tone: 'success',
  },
  {
    time: '13:58',
    title: 'Wind down: idle Claude session on Target A',
    detail: 'idle 16 min (limit 15) · at a safe point · keep warm 0',
    outcome: 'Stopped',
    tone: 'neutral',
  },
];

/**
 * The Scheduler section: the mode, the ranked plan, one explanation, resource contention, and the
 * decision log (§26.17, §26.20). Selecting a plan row selects the same step in the inspector.
 */
export function OrchestrationMockScheduler({
  scenario,
  selectedStep,
}: {
  scenario: MockScenario;
  selectedStep: string;
}) {
  const entries = plan(scenario);
  return (
    <List gap="l">
      <StateBanner
        copyLayout="stacked"
        tone="info"
        title="Dry Run: Automatic Placement Is Off"
        detail="The plan below is computed and logged. Nothing starts or stops until the policy turns placement on."
        action={
          <Button size="compact" shape="pill">
            Open Policy Settings
          </Button>
        }
      />
      <section aria-label="Plan">
        <List gap="xs">
          <ListHeader label="Plan" status="Next change at 14:20" />
          <List selectionMode="single" ariaLabel="Ranked Work">
            {entries.map((entry, index) => (
              <SummaryRow
                title={<Text variant="span">{`${index + 1}. ${entry.title}`}</Text>}
                trailingMeta={`Score ${entry.score}`}
                selected={entry.step === selectedStep}
                listEdge={index === 0 ? 'start' : index === entries.length - 1 ? 'end' : undefined}
                action={ORCH_MOCK_SELECT_STEP}
                rootAttributes={{ 'data-step-id': entry.step }}
                metadata={
                  <Row gap="xs" wrap vAlign="middle">
                    <Badge tone={entry.tone} size="compact">
                      {entry.decision}
                    </Badge>
                    <Text tone="quiet" size="compact" flush>
                      {entry.reason}
                    </Text>
                  </Row>
                }
              />
            ))}
          </List>
        </List>
      </section>
      <section aria-label="Resource Contention">
        <List gap="xs">
          <ListHeader label="Resource Contention" />
          <ValueTable label="Resource contention" density="compact" stackBelow="narrow">
            <ValueTableRow label="build-cache · Build host 2" value="Held until 14:20 · given to TB-17 · TB-20 waits" />
            <ValueTableRow
              label="staging-db · Build host 3"
              value={scenario === 'degraded' ? 'Unreadable · backend unreachable' : 'Free'}
            />
          </ValueTable>
        </List>
      </section>
      <section aria-label="Decision Log">
        <List gap="xs">
          <ListHeader label="Decision Log" />
          <List ariaLabel="Scheduler Decisions">
            {decisions.map((decision, index) => (
              <SummaryRow
                semanticRole="group"
                ariaLabel={`${decision.time} ${decision.title}`}
                title={<Text variant="span">{decision.title}</Text>}
                trailingMeta={decision.time}
                listEdge={index === 0 ? 'start' : index === decisions.length - 1 ? 'end' : undefined}
                metadata={
                  <Row gap="xs" wrap vAlign="middle">
                    <Badge tone={decision.tone} size="compact">
                      {decision.outcome}
                    </Badge>
                    <Text tone="quiet" size="compact" flush>
                      {decision.detail}
                    </Text>
                  </Row>
                }
              />
            ))}
          </List>
        </List>
      </section>
    </List>
  );
}
