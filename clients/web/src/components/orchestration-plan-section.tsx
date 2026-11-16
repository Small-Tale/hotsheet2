import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';

import { readinessLevelText, readinessUnblocks, readinessWhy } from '../orchestration-plan';
import type { OrchestrationTimeline, TimelineItem } from '../orchestration-timeline';
import { OrchestrationInspectorSection } from './orchestration-inspector-section';

export interface OrchestrationPlanSectionProps {
  item: TimelineItem;
  timeline: OrchestrationTimeline;
}

/**
 * HS2-15G5T4 (design HS2-2XJSJK): the card's place in the readiness plan (docs/26 §26.15): its level,
 * why it stands there in words (a cycle reads as a reason), and the waiting steps it unblocks. A card
 * whose Prerequisites section already lists what it waits on leaves those out of Why.
 */
export function OrchestrationPlanSection({ item, timeline }: OrchestrationPlanSectionProps) {
  const why = readinessWhy(item, { omitPrerequisites: item.state === 'waits_for_prerequisite' }),
    unblocks = readinessUnblocks(item, timeline);
  return (
    <OrchestrationInspectorSection label="Plan">
      <ValueTable label="Plan" density="compact" stackBelow="narrow">
        <ValueTableRow label="Readiness" value={readinessLevelText(item.level)} />
        {why.length > 0 ? <ValueTableRow label="Why" value={why.join('. ')} /> : undefined}
        <ValueTableRow label="Unblocks" value={unblocks.length > 0 ? unblocks.join(', ') : 'Nothing waits on it'} />
      </ValueTable>
    </OrchestrationInspectorSection>
  );
}
