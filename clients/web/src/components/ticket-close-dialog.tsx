import '@awesome.me/webawesome/dist/components/input/input.js';
import './ticket-close-dialog.css';

import { Button } from '@kerfjs/ui/button';
import { rem, remify, uiColor } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { Spacer } from '@kerfjs/ui/spacer';
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
                  <wa-input
                    name="ticket-close-target-search"
                    label="Existing Ticket"
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
                        <Button
                          type="button"
                          emphasis="plain"
                          size="compact"
                          action={TICKET_SELECTION_ACTIONS.clearTicketCloseTarget.value}
                        >
                          Change
                        </Button>
                      }
                    />
                  )}
                  {!state.selected && Boolean(state.query.trim()) && (
                    // Kerf's framed, bounded result picker (KF-SHW2J9, HS2-Z46VA7) owns the border,
                    // radius, focus-ring inset, and min/max scroll bounds.
                    <List
                      scrollable
                      frame
                      minHeight={rem(4.25)}
                      maxHeight={rem(14)}
                      className="ticket-close-dialog__results"
                      rootAttributes={{
                        'aria-label': 'Matching tickets',
                        'aria-busy': String(Boolean(state.searching)),
                      }}
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
                    </List>
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
