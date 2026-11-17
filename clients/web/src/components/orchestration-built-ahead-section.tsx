import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu } from '@kerfjs/ui/popup-menu';
import { Row } from '@kerfjs/ui/row';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { RotateCcw } from 'lucide';

import { ORCHESTRATION_ACTIONS } from '../interaction-attrs/orchestration';
import {
  atRiskDetail,
  builtAheadRows,
  type BuiltAheadView,
  canReopen,
  REOPEN_REASONS,
} from '../orchestration-built-ahead';
import type { OrchestrationTimeline } from '../orchestration-timeline';
import { OrchestrationInspectorSection } from './orchestration-inspector-section';

export interface OrchestrationBuiltAheadSectionProps {
  /** The recorded work; absent while it is read or when nothing was recorded. */
  work?: BuiltAheadView;
  timeline: OrchestrationTimeline;
  busy?: boolean;
}

/**
 * HS2-0D60T8 (design HS2-2XJSJK, answer A): work built ahead of a pending gate. The gate and commit it
 * rests on, its state and depth; an At Risk banner naming what to repair first; and Reopen… for a
 * person, with a reason code, when the fix invalidated it. Without a record yet it says only what the
 * card is.
 */
export function OrchestrationBuiltAheadSection({ work, timeline, busy = false }: OrchestrationBuiltAheadSectionProps) {
  if (!work)
    return (
      <OrchestrationInspectorSection
        label="Built Ahead"
        note="This work rests on a prerequisite whose CI is still running. If that CI fails, it is held back from shipping."
      />
    );
  return (
    <OrchestrationInspectorSection label="Built Ahead">
      {work.state === 'at_risk' ? (
        <StateBanner copyLayout="stacked" tone="warning" title="At Risk" detail={atRiskDetail(work, timeline)} />
      ) : undefined}
      <ValueTable label="Built ahead" density="compact" stackBelow="narrow">
        {builtAheadRows(work, timeline).map((row) => (
          <ValueTableRow label={row.label} value={row.value} />
        ))}
      </ValueTable>
      {canReopen(work) ? (
        <Row gap="xs" wrap controlInsets="rl">
          <PopupMenu
            text="Reopen…"
            size="compact"
            emphasis="secondary"
            disabled={busy}
            icon={<LucideIcon size="s" icon={RotateCcw} name="rotate-ccw" />}
            items={REOPEN_REASONS.map((reason) => ({
              label: reason.label,
              icon: <LucideIcon icon={RotateCcw} name="rotate-ccw" />,
              action: ORCHESTRATION_ACTIONS.reopenOrchestrationWork.value,
              attributes: { 'data-reopen-reason': reason.code },
            }))}
          />
        </Row>
      ) : undefined}
    </OrchestrationInspectorSection>
  );
}
