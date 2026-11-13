import type { KerfUiContent } from '@kerfjs/ui';
import { Badge } from '@kerfjs/ui/badge';
import { Button } from '@kerfjs/ui/button';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { Row } from '@kerfjs/ui/row';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';

import { inspectorToggle, type SidebarPanelParts } from '../../components/sidebar-panel';
import { LEVEL_PRESENTATION, type MockScenario, type MockStep } from './fixtures';

export const ORCH_MOCK_RUN_CONTROL = 'orch-mock-run-control';

/** A labeled inspector section: a ListHeader over its facts. */
export function MockSection({ label, children }: { label: string; children?: KerfUiContent }) {
  return (
    <section aria-label={label}>
      <List gap="xs">
        <ListHeader label={label} />
        {children}
      </List>
    </section>
  );
}

function RunControls({ paused }: { paused: boolean }) {
  return (
    <Row gap="xs" wrap controlInsets="rl">
      {paused ? (
        <Button size="compact" action={ORCH_MOCK_RUN_CONTROL} rootAttributes={{ 'data-control': 'resume' }}>
          Resume
        </Button>
      ) : (
        <Button size="compact" action={ORCH_MOCK_RUN_CONTROL} rootAttributes={{ 'data-control': 'pause' }}>
          Pause
        </Button>
      )}
      <Button size="compact" action={ORCH_MOCK_RUN_CONTROL} rootAttributes={{ 'data-control': 'handoff' }}>
        Hand Off…
      </Button>
      <Button size="compact" action={ORCH_MOCK_RUN_CONTROL} rootAttributes={{ 'data-control': 'correct' }}>
        Correct…
      </Button>
      <Button size="compact" tone="danger" action={ORCH_MOCK_RUN_CONTROL} rootAttributes={{ 'data-control': 'cancel' }}>
        Cancel Run…
      </Button>
    </Row>
  );
}

/** A dispatched run: status, last confirmed target state, events, reconciliation, and controls. */
function RunDetail({ step, scenario }: { step: MockStep; scenario: MockScenario }) {
  const unreachable = scenario === 'degraded' && step.target === 'Target C';
  return (
    <>
      <MockSection label="Run">
        {unreachable && (
          <StateBanner
            copyLayout="stacked"
            tone="warning"
            title="Target C Cannot Be Confirmed"
            detail="Unreachable since 14:02. The state below is the last one confirmed, 12 minutes ago."
          />
        )}
        <ValueTable label="Run status" density="compact" stackBelow="narrow">
          <ValueTableRow label="State" value={`Dispatched to ${step.run?.worker ?? 'a worker'}`} />
          <ValueTableRow
            label="Lease"
            value={
              unreachable
                ? 'Unconfirmed while the target is unreachable'
                : step.run?.lease
                  ? `${step.run.lease.replace('Lease ', '')} left`
                  : 'None'
            }
          />
          <ValueTableRow
            label="Target ticket"
            value={`${step.slug} on ${step.target}: Started, working, claimed by ${step.run?.worker ?? '—'}${unreachable ? ' (as last confirmed)' : ''}`}
          />
          <ValueTableRow label="Last confirmed" value={unreachable ? '12 min ago (stale)' : '40 s ago'} />
          <ValueTableRow label="Session" value={step.session ?? 'None'} />
        </ValueTable>
        <RunControls paused={step.paused === true} />
      </MockSection>
      <MockSection label="Gates">
        <ValueTable label="Gates" density="compact" stackBelow="narrow">
          <ValueTableRow
            label="CI · build"
            value={
              step.gate === 'failed'
                ? 'Failed (run 4812, attempt 2)'
                : step.gate === 'unknown'
                  ? 'Unknown · watch lost'
                  : 'Pending (run 4812)'
            }
          />
          <ValueTableRow label="Deployment" value="None requested" />
        </ValueTable>
      </MockSection>
      <MockSection label="Validation">
        <ValueTable label="Validation steps" density="compact" stackBelow="narrow">
          <ValueTableRow label="Unit tests" value="Passed · evidence 7c1e…" />
          <ValueTableRow label="Contract check" value="Running since 14:08" />
        </ValueTable>
      </MockSection>
      <MockSection label="Reconciliation">
        <ValueTable label="Reconciliation findings" density="compact" stackBelow="narrow">
          <ValueTableRow label="Disposition" value={unreachable ? 'Hold: unverified' : 'Continue'} />
          <ValueTableRow
            label="Target reachability"
            value={unreachable ? 'Unverifiable · target_unavailable' : 'Verified'}
          />
          <ValueTableRow label="Target ticket and claim" value={unreachable ? 'Unverifiable' : 'Verified'} />
          <ValueTableRow label="Run lease" value={unreachable ? 'Unverifiable' : 'Verified'} />
          <ValueTableRow label="CI, deployment, session" value="Not wired yet" />
        </ValueTable>
      </MockSection>
      <MockSection label="Events">
        <ValueTable label="Run events" density="compact" stackBelow="narrow">
          <ValueTableRow label="14:08" value="Validation: Contract check running · orchestrator" />
          <ValueTableRow label="14:07" value="Gate: CI build pending · learned from target" />
          <ValueTableRow label="14:05" value="Session started: Codex on Build host 2 · orchestrator" />
          <ValueTableRow label="14:05" value="State: queued → dispatched · learned from target" />
          <ValueTableRow label="14:01" value="State: requested → queued · orchestrator" />
        </ValueTable>
      </MockSection>
    </>
  );
}

/** Work that is not running: the scheduler's explanation and what would change it. */
function NotRunning({ step, scenario }: { step: MockStep; scenario: MockScenario }) {
  const approval = step.id === 'step-ta47',
    resource = step.id === 'step-tb20';
  return (
    <>
      {approval && (
        <StateBanner
          copyLayout="stacked"
          tone="warning"
          title="Waiting for Your Approval"
          detail="The policy asks before a session starts."
          action={
            <Row gap="xs" wrap>
              <Button size="compact" shape="pill">
                Deny
              </Button>
              <Button size="compact" shape="pill" tone="brand">
                Approve
              </Button>
            </Row>
          }
        />
      )}
      <MockSection label="Why it is not running">
        <ValueTable label="Scheduler explanation" density="compact" stackBelow="narrow">
          <ValueTableRow
            label="Answer"
            value={
              approval
                ? 'Not placed · approval_required'
                : resource
                  ? 'Not placed · resource_held'
                  : step.level === 'confirmed'
                    ? 'Start · next pass'
                    : 'Not ready'
            }
          />
          <ValueTableRow label="Detail" value={step.next ?? '—'} />
          <ValueTableRow label="Rank" value={approval ? '1 of 4' : resource ? '2 of 4' : '3 of 4'} />
          <ValueTableRow
            label="Expect a change"
            value={resource ? 'At 14:20, or when TB-17 releases it' : 'On your answer'}
          />
        </ValueTable>
      </MockSection>
      <MockSection label="Score">
        <ValueTable label="Score terms" density="compact" stackBelow="narrow">
          <ValueTableRow label="Priority (high)" value="3,000" />
          <ValueTableRow label="Up Next" value="1,500" />
          <ValueTableRow label="Unblocks 1" value="200" />
          <ValueTableRow label="Aging (52 min)" value="866" />
          <ValueTableRow label="Total" value="5,566" />
        </ValueTable>
      </MockSection>
      {resource && (
        <MockSection label="Resources">
          <ValueTable label="Required resources" density="compact" stackBelow="narrow">
            <ValueTableRow label="build-cache" value="Held on Build host 2 by TB-17 until 14:20 · declared by step" />
          </ValueTable>
        </MockSection>
      )}
      {scenario === 'degraded' && step.level === 'blocked' && (
        <Text tone="quiet" size="compact">
          A person can request a readiness override for this step; it is filed as an approval.
        </Text>
      )}
    </>
  );
}

/** The right rail for a selected plan step: its run when one exists, otherwise why it is not running. */
export function orchestrationMockStepPanel({
  step,
  scenario,
}: {
  step: MockStep | undefined;
  scenario: MockScenario;
}): SidebarPanelParts {
  const level = step ? LEVEL_PRESENTATION[step.level] : undefined;
  return {
    label: 'Orchestration Inspector',
    toolbar: {
      label: 'Orchestration Inspector Toolbar',
      dividerSides: '',
      center: <ToolbarText text={step ? `${step.slug} on ${step.target}` : 'Nothing selected'} size="small" />,
    },
    toggle: inspectorToggle('Orchestration Inspector'),
    content: step ? (
      <List gap="l" controlInsets="tb" rootAttributes={{ 'data-orch-mock-inspector': step.id }}>
        <List gap="xs" textInsets="rl">
          <Text variant="h2" size="medium" flush>
            {step.title}
          </Text>
          <Row gap="xs" wrap>
            {level && (
              <Badge tone={level.tone} size="compact">
                {level.label}
              </Badge>
            )}
            {step.unblocks > 0 && (
              <Badge tone="neutral" appearance="outline" size="compact">
                {`Unblocks ${step.unblocks}`}
              </Badge>
            )}
          </Row>
          {step.levelReason ? (
            <Text tone="quiet" size="compact" flush>
              {step.levelReason}
            </Text>
          ) : undefined}
        </List>
        {step.run && step.run.state !== 'completed' ? (
          <RunDetail step={step} scenario={scenario} />
        ) : step.level === 'finished' ? (
          <MockSection label="Result">
            <ValueTable label="Finished run" density="compact" stackBelow="narrow">
              <ValueTableRow label="State" value="Completed" />
              <ValueTableRow label="Lead time" value="1 h 52 min" />
              <ValueTableRow label="CI · build" value="Passed (run 4790)" />
              <ValueTableRow label="Validation" value="2 of 2 passed" />
            </ValueTable>
          </MockSection>
        ) : (
          <NotRunning step={step} scenario={scenario} />
        )}
      </List>
    ) : (
      <List fill hAlign="center" vAlign="middle">
        <Text tone="quiet">Select work to see its run or why it is not running</Text>
      </List>
    ),
    pane: {},
  };
}
