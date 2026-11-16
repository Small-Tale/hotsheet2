import './ticket-close-dialog.css';

import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { Spacer } from '@kerfjs/ui/spacer';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { CopyX } from 'lucide';

import { TICKET_SELECTION_ACTIONS, TICKET_SELECTION_TARGETS } from '../interaction-attrs/ticket-selection';
import {
  type DuplicateTarget,
  duplicateTargetKey,
  TICKET_CLOSE_REASON_CHOICES,
  type TicketCloseReason,
  validateTicketClose,
} from '../ticket-close';
import { TicketReferencePicker } from './ticket-reference-picker';

export interface TicketCloseDialogState {
  source: DuplicateTarget;
  reason: TicketCloseReason;
  query: string;
  candidates: readonly DuplicateTarget[];
  selected?: DuplicateTarget;
  searching?: boolean;
  submitting?: boolean;
  error?: string;
}

export function TicketCloseDialog({ state }: { state?: TicketCloseDialogState }) {
  if (!state) return <></>;
  const duplicate = state.reason === 'duplicate';
  const validation = validateTicketClose(state.reason, state.source, state.selected);
  const candidates = state.candidates.filter(
    (candidate) => duplicateTargetKey(candidate) !== duplicateTargetKey(state.source),
  );
  return (
    <div class="ticket-close-dialog" data-key="ticket-close-dialog">
      <DialogSurface
        label={`Close ${state.source.slug}`}
        preferredWidth={remify(544)}
        viewportGutter={remify(16)}
        rootAttributes={{
          'data-key': 'ticket-close-dialog',
          ...TICKET_SELECTION_TARGETS.ticketCloseDialog.attrs,
          'data-controlled-open': 'true',
        }}
      >
        <form {...TICKET_SELECTION_ACTIONS.submitTicketClose.attrs}>
          <List gap="m">
            <Text tone="quiet">
              Record why this ticket is being closed so the outcome remains searchable and unambiguous.
            </Text>
            <Select
              name="ticket-close-reason"
              value={state.reason}
              label="Close As"
              choices={TICKET_CLOSE_REASON_CHOICES}
            />
            {duplicate && (
              <section aria-label="Duplicate Target">
                <List gap="xs">
                  <TicketReferencePicker
                    fieldName="ticket-close-target-search"
                    query={state.query}
                    candidates={candidates}
                    selected={state.selected}
                    searching={state.searching}
                    selectAction={TICKET_SELECTION_ACTIONS.selectTicketCloseTarget.value}
                    clearAction={TICKET_SELECTION_ACTIONS.clearTicketCloseTarget.value}
                    candidateIcon={<LucideIcon icon={CopyX} name="copy-x" />}
                  />
                  <Text size="compact" tone="quiet">
                    The selected ticket becomes the canonical target. This relationship is stored as structured
                    duplicate metadata.
                  </Text>
                </List>
              </section>
            )}
            <div class="ticket-close-dialog__error" role="alert">
              {(state.error || validation) && (
                <Text size="compact" tone="danger">
                  {state.error || validation}
                </Text>
              )}
            </div>
            {/* The same footer layout as the project close dialog: Cancel leads, the decision trails (HS2-0DHVGK). */}
            <Row vAlign="middle" gap="xs">
              <Button
                type="button"
                action={TICKET_SELECTION_ACTIONS.cancelTicketClose.value}
                emphasis="secondary"
                disabled={state.submitting}
              >
                Cancel
              </Button>
              <Spacer flex />
              <Button type="submit" tone={'brand'} disabled={Boolean(validation) || state.submitting}>
                {state.submitting ? 'Closing…' : duplicate ? 'Mark as duplicate' : 'Close ticket'}
              </Button>
            </Row>
          </List>
        </form>
      </DialogSurface>
    </div>
  );
}
