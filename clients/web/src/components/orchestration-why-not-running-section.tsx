import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';

import { resourceRows } from '../orchestration-resources';
import { explanationRows, type SchedulerExplanation } from '../orchestration-scheduler';
import type { OrchestrationTimeline, TimelineItem } from '../orchestration-timeline';
import { type ClockFormat, machinePhrase, signedOutText, signedOutTool } from '../orchestration-timeline-board';
import { OrchestrationInspectorSection } from './orchestration-inspector-section';

export interface OrchestrationWhyNotRunningSectionProps {
  item: TimelineItem;
  /** For the slugs of steps a resource was given to. */
  timeline?: OrchestrationTimeline;
  /** `scheduler explain` for the card; absent until it is read. */
  explanation?: SchedulerExplanation;
  clock: ClockFormat;
}

const words = (code: string) => code.replaceAll('_', ' ');

/** HS2-ZRPZJJ: the last pass's reason; a signed-out tool is named with its machine. */
function lastPass(item: TimelineItem): string {
  const placement = item.placement;
  const tool = signedOutTool(placement?.detail);
  return tool ? signedOutText(tool, machinePhrase(item)) : words(placement?.reason ?? '');
}

/**
 * HS2-77SRHC (design HS2-2XJSJK): why a ready card is not running, from the scheduler's own explanation:
 * what it decided at its last pass and when it looks again. Until the explanation is read, the
 * scheduler's last logged reason for the step stands in.
 */
export function OrchestrationWhyNotRunningSection({
  item,
  timeline,
  explanation,
  clock,
}: OrchestrationWhyNotRunningSectionProps) {
  if (!explanation)
    return (
      <OrchestrationInspectorSection
        label="Why It Is Not Running"
        note={
          item.placement?.reason
            ? `The scheduler's last pass: ${lastPass(item)}.`
            : 'It is ready; it starts when a session is free for it.'
        }
      />
    );
  return (
    <OrchestrationInspectorSection label="Why It Is Not Running">
      <ValueTable label="Why it is not running" density="compact" stackBelow="narrow">
        {[
          ...explanationRows(explanation, clock, item.placement?.at_ms, timeline, item),
          // HS2-5GRXFN: the resources the work needs and any that is not free.
          ...resourceRows(explanation.resources ?? [], timeline, clock),
        ].map((row) => (
          <ValueTableRow label={row.label} value={row.value} />
        ))}
      </ValueTable>
    </OrchestrationInspectorSection>
  );
}
