import { Button } from '@kerfjs/ui/button';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';

import { ORCHESTRATION_ACTIONS } from '../interaction-attrs/orchestration';
import { decisionText, type SchedulerDecision } from '../orchestration-scheduler';
import type { OrchestrationTimeline } from '../orchestration-timeline';
import { type ClockFormat, clockTime } from '../orchestration-timeline-board';

/** The open Show All Decisions… dialog: the pages read so far, newest first. */
export interface OrchestrationDecisionsDialogState {
  entries: readonly SchedulerDecision[];
  /** Older decisions are retained beyond what was read. */
  more: boolean;
  loading: boolean;
  error?: string;
}

export interface OrchestrationDecisionsDialogProps {
  state?: OrchestrationDecisionsDialogState;
  timeline?: OrchestrationTimeline;
  clock?: ClockFormat;
  /** The UX catalog shows the dialog in place; the app opens it over the view. */
  presentation?: 'embedded';
}

/**
 * HS2-77SRHC (design HS2-2XJSJK answer A): the scheduler's whole decision log, newest first, in a Kerf
 * `DialogSurface`, dismissed by its own close control or Escape. Load Earlier reads the page before the
 * oldest one shown; nothing reads on a timer.
 */
export function OrchestrationDecisionsDialog({
  state,
  timeline,
  clock = clockTime,
  presentation,
}: OrchestrationDecisionsDialogProps) {
  if (!state) return <></>;
  return (
    <div data-key="orchestration-decisions-dialog">
      <DialogSurface
        label="Scheduler Decisions"
        presentation={presentation}
        rootAttributes={{
          'data-key': 'orchestration-decisions-dialog',
          'data-dialog-component': 'orchestration-decisions-dialog',
          'data-controlled-open': 'true',
        }}
        footer={
          state.more ? (
            <Row vAlign="middle" gap="xs">
              <Button
                emphasis="secondary"
                type="button"
                disabled={state.loading}
                action={ORCHESTRATION_ACTIONS.loadEarlierOrchestrationDecisions.value}
              >
                Load Earlier
              </Button>
            </Row>
          ) : undefined
        }
      >
        <List gap="m">
          {state.entries.length > 0 ? (
            <ValueTable label="Scheduler decisions" density="compact" stackBelow="narrow">
              {state.entries.map((decision) => (
                <ValueTableRow
                  label={clock(decision.at_ms)}
                  value={`${decisionText(decision, timeline)}${decision.detail ? ` (${decision.detail})` : ''}`}
                />
              ))}
            </ValueTable>
          ) : (
            <Text tone="quiet" flush>
              {state.loading ? 'Reading the decision log…' : 'The scheduler has decided nothing yet.'}
            </Text>
          )}
          {state.error ? (
            <Text tone="danger" role="alert" flush>
              {state.error}
            </Text>
          ) : undefined}
        </List>
      </DialogSurface>
    </div>
  );
}
