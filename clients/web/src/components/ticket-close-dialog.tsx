import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import './ticket-close-dialog.css';

import { remify, uiColor } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { CheckCircle2, CopyX, Search } from 'lucide';

import { TICKET_SELECTION_ACTIONS, TICKET_SELECTION_TARGETS } from '../interaction-attrs/ticket-selection';
import {
  type DuplicateTarget,
  duplicateTargetKey,
  TICKET_CLOSE_REASON_CHOICES,
  type TicketCloseReason,
  validateTicketClose,
} from '../ticket-close';

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
    <div data-key="ticket-close-dialog">
      <DialogSurface preferredWidth={remify(544)} viewportGutter={remify(16)}>
        <wa-dialog
          data-key="ticket-close-dialog"
          class="ticket-close-dialog"
          open
          {...TICKET_SELECTION_TARGETS.ticketCloseDialog.attrs}
          label={`Close ${state.source.slug}`}
        >
          <form {...TICKET_SELECTION_ACTIONS.submitTicketClose.attrs}>
            <List gap="m">
              <Text tone="quiet">
                Record why this ticket is being closed so the outcome remains searchable and unambiguous.
              </Text>
              <Select
                name="ticket-close-reason"
                value={state.reason}
                label="Close as"
                choices={TICKET_CLOSE_REASON_CHOICES}
              />
              {duplicate && (
                <section aria-label="Duplicate target">
                  <List gap="xs">
                    <wa-input
                      name="ticket-close-target-search"
                      label="Existing ticket"
                      placeholder="Search by ticket number or title"
                      value={state.query}
                    >
                      <LucideIcon
                        slot="start"
                        icon={Search}
                        name="search"
                        size={16}
                        color={uiColor('neutral-on-quiet')}
                      />
                    </wa-input>
                    {state.selected && (
                      <StateBanner
                        tone="success"
                        icon={<LucideIcon icon={CheckCircle2} name="check-circle-2" />}
                        title={state.selected.slug}
                        badge={state.selected.projectName}
                        detail={state.selected.title}
                        copyLayout="stacked"
                        action={
                          <wa-button
                            type="button"
                            appearance="plain"
                            size="small"
                            {...TICKET_SELECTION_ACTIONS.clearTicketCloseTarget.attrs}
                          >
                            Change
                          </wa-button>
                        }
                      />
                    )}
                    {!state.selected && Boolean(state.query.trim()) && (
                      <div
                        class="ticket-close-dialog__results"
                        aria-label="Matching tickets"
                        aria-busy={String(Boolean(state.searching))}
                      >
                        {candidates.map((candidate) => (
                          <ListItem
                            action="select-ticket-close-target"
                            itemId={duplicateTargetKey(candidate)}
                            icon={<LucideIcon icon={CopyX} name="copy-x" />}
                            label={
                              <>
                                {candidate.slug}{' '}
                                <Text variant="span" size="compact" tone="quiet">
                                  {candidate.projectName}
                                </Text>
                              </>
                            }
                            description={
                              <Text variant="span" wrap="truncate">
                                {candidate.title}
                              </Text>
                            }
                            multiline
                          />
                        ))}
                        {!state.searching && candidates.length === 0 && <Text tone="quiet">No matching tickets.</Text>}
                      </div>
                    )}
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
              <Row hAlign="right" vAlign="middle" gap="xs">
                <wa-button
                  type="button"
                  {...TICKET_SELECTION_ACTIONS.cancelTicketClose.attrs}
                  appearance="outlined"
                  disabled={state.submitting}
                >
                  Cancel
                </wa-button>
                <wa-button type="submit" variant="brand" disabled={Boolean(validation) || state.submitting}>
                  {state.submitting ? 'Closing…' : duplicate ? 'Mark as duplicate' : 'Close ticket'}
                </wa-button>
              </Row>
            </List>
          </form>
        </wa-dialog>
      </DialogSurface>
    </div>
  );
}
