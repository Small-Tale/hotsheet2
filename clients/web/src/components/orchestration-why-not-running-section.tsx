import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';

import { explanationRows, type SchedulerExplanation } from '../orchestration-scheduler';
import type { TimelineItem } from '../orchestration-timeline';
import type { ClockFormat } from '../orchestration-timeline-board';
import { OrchestrationInspectorSection } from './orchestration-inspector-section';

export interface OrchestrationWhyNotRunningSectionProps {
  item: TimelineItem;
  /** `scheduler explain` for the card; absent until it is read. */
  explanation?: SchedulerExplanation;
  clock: ClockFormat;
}

const words = (code: string) => code.replaceAll('_', ' ');

/**
 * HS2-77SRHC (design HS2-2XJSJK): why a ready card is not running, from the scheduler's own explanation:
 * what it decided at its last pass and when it looks again. Until the explanation is read, the
 * scheduler's last logged reason for the step stands in.
 */
export function OrchestrationWhyNotRunningSection({
  item,
  explanation,
  clock,
}: OrchestrationWhyNotRunningSectionProps) {
  if (!explanation)
    return (
      <OrchestrationInspectorSection
        label="Why It Is Not Running"
        note={
          item.placement?.reason
            ? `The scheduler's last pass: ${words(item.placement.reason)}.`
            : 'It is ready; it starts when a session is free for it.'
        }
      />
    );
  return (
    <OrchestrationInspectorSection label="Why It Is Not Running">
      <ValueTable label="Why it is not running" density="compact" stackBelow="narrow">
        {explanationRows(explanation, clock, item.placement?.at_ms).map((row) => (
          <ValueTableRow label={row.label} value={row.value} />
        ))}
      </ValueTable>
    </OrchestrationInspectorSection>
  );
}
