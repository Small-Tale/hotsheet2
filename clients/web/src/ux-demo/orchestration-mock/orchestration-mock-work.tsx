import { Badge } from '@kerfjs/ui/badge';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { SummaryRow } from '@kerfjs/ui/summary-row';
import { Text } from '@kerfjs/ui/text';
import { CircleCheck, CircleDashed, CircleDot, CloudOff, OctagonX, Play } from 'lucide';

import { TicketGroup } from '../../components/ticket-group';
import {
  LEVEL_PRESENTATION,
  type MockGrouping,
  type MockOrchestrationTicket,
  type MockStep,
  type ReadinessLevel,
  type Verdict,
  VERDICT_PRESENTATION,
} from './fixtures';

export const ORCH_MOCK_SELECT_STEP = 'orch-mock-select-step';

const LEVEL_ICONS: Record<ReadinessLevel, [typeof Play, string]> = {
  confirmed: [CircleDot, 'circle-dot'],
  provisional: [CircleDashed, 'circle-dashed'],
  blocked: [OctagonX, 'octagon-x'],
  stale: [CloudOff, 'cloud-off'],
  in_progress: [Play, 'play'],
  finished: [CircleCheck, 'circle-check'],
};

function verdictBadge(kind: 'CI' | 'Checks', verdict: Verdict) {
  if (verdict === 'none') return undefined;
  const presentation = VERDICT_PRESENTATION[verdict];
  return (
    <Badge tone={presentation.tone} size="compact">
      {`${kind} ${presentation.label}`}
    </Badge>
  );
}

/** One plan step (a target ticket and its run) as a selectable summary row. */
function StepRow({
  step,
  ticket,
  selected,
  listEdge,
}: {
  step: MockStep;
  ticket?: MockOrchestrationTicket;
  selected: boolean;
  listEdge?: 'start' | 'end' | 'only';
}) {
  const level = LEVEL_PRESENTATION[step.level],
    [icon, iconName] = LEVEL_ICONS[step.level];
  return (
    <SummaryRow
      title={<Text variant="span">{`${step.slug} ${step.title}`}</Text>}
      leading={<LucideIcon icon={icon} name={iconName} />}
      trailingMeta={step.run?.lease ?? step.updated}
      selected={selected}
      listEdge={listEdge}
      rail={step.level === 'blocked' ? 'danger' : step.level === 'stale' ? 'warning' : undefined}
      action={ORCH_MOCK_SELECT_STEP}
      rootAttributes={{ 'data-step-id': step.id }}
      metadata={
        <List gap="xs">
          <Row gap="xs" wrap>
            {ticket && (
              <Badge tone="neutral" appearance="outline" size="compact">
                {ticket.slug}
              </Badge>
            )}
            <Badge tone="neutral" size="compact">
              {step.target}
            </Badge>
            <Badge tone={level.tone} size="compact">
              {level.label}
            </Badge>
            {verdictBadge('CI', step.gate)}
            {verdictBadge('Checks', step.validation)}
            {step.session ? (
              <Badge tone="neutral" appearance="outline" size="compact">
                {step.session}
              </Badge>
            ) : undefined}
            {step.unblocks > 0 && (
              <Badge tone="neutral" appearance="outline" size="compact">
                {`Unblocks ${step.unblocks}`}
              </Badge>
            )}
          </Row>
          {(step.levelReason ?? step.next) !== undefined && (
            <Text tone="quiet" size="compact" flush>
              {[step.levelReason, step.next].filter(Boolean).join(' · ')}
            </Text>
          )}
        </List>
      }
    />
  );
}

const edge = (index: number, length: number): 'start' | 'end' | 'only' | undefined =>
  length === 1 ? 'only' : index === 0 ? 'start' : index === length - 1 ? 'end' : undefined;

/** Active work first, then ready, waiting, and finished work. */
const FLAT_ORDER: readonly ReadinessLevel[] = [
  'in_progress',
  'blocked',
  'stale',
  'confirmed',
  'provisional',
  'finished',
];

/**
 * The Work section: plan steps grouped under their orchestration ticket (the maintainer's leaning),
 * or one flat list for comparison. Grouping reuses the app's ticket-group container.
 */
export function OrchestrationMockWork({
  tickets,
  grouping,
  selectedStep,
  collapsed,
}: {
  tickets: readonly MockOrchestrationTicket[];
  grouping: MockGrouping;
  selectedStep: string;
  collapsed: readonly string[];
}) {
  if (grouping === 'flat') {
    const rows = tickets
      .flatMap((ticket) => ticket.steps.map((step) => ({ ticket, step })))
      .sort((a, b) => FLAT_ORDER.indexOf(a.step.level) - FLAT_ORDER.indexOf(b.step.level));
    return (
      <List selectionMode="single" ariaLabel="Orchestrated Work">
        {rows.map(({ ticket, step }, index) => (
          <StepRow
            step={step}
            ticket={ticket}
            selected={step.id === selectedStep}
            listEdge={edge(index, rows.length)}
          />
        ))}
      </List>
    );
  }
  return (
    <List gap="m">
      {tickets.map((ticket) => (
        <TicketGroup
          group={ticket.slug}
          name={`${ticket.slug} ${ticket.title}`}
          count={ticket.steps.length}
          scope="orchestration-mock"
          collapsed={collapsed.includes(ticket.slug)}
        >
          <List selectionMode="single" ariaLabel={`${ticket.slug} Plan Steps`}>
            {ticket.steps.map((step, index) => (
              <StepRow step={step} selected={step.id === selectedStep} listEdge={edge(index, ticket.steps.length)} />
            ))}
          </List>
        </TicketGroup>
      ))}
    </List>
  );
}
