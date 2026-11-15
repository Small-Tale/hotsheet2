import './ticket-blocker-dialog.css';

import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Spacer } from '@kerfjs/ui/spacer';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { Ban } from 'lucide';

import { INSPECTOR_AND_EDITOR_ACTIONS, INSPECTOR_AND_EDITOR_TARGETS } from '../interaction-attrs/inspector-and-editor';
import { blockerCandidates, type BlockerTicket, validateBlocker } from '../ticket-blockers';
import { TicketReferencePicker } from './ticket-reference-picker';

export interface TicketBlockerDialogState {
  /** The inspected ticket that will wait on the chosen blocker. */
  source: BlockerTicket;
  /** The native ids already in the source's `blocked_by`. */
  existing: readonly string[];
  query: string;
  candidates: readonly BlockerTicket[];
  selected?: BlockerTicket;
  searching?: boolean;
  submitting?: boolean;
  error?: string;
  /**
   * The reader frame that opened the dialog, which renders it inside that modal reader so it stays
   * interactive (HS2-15PE59); absent when the Inspector opened it at the app root.
   */
  readerScope?: string;
}

/**
 * Add a dependency blocker to the inspected ticket (HS2-CHH28E). The Blocked By header's Add
 * Blocker action opens it. The person searches the inspected project with the shared
 * {@link TicketReferencePicker}; only tickets from the same ticket source are offered because
 * `blocked_by` is source-local. A self-block, an existing blocker, or a server refusal such as
 * a dependency cycle stays in the dialog.
 */
export function TicketBlockerDialog({ state }: { state?: TicketBlockerDialogState }) {
  if (!state) return <></>;
  const validation = validateBlocker(state.source, state.selected, state.existing),
    candidates = blockerCandidates(state.source, state.candidates);
  return (
    <div class="ticket-blocker-dialog" data-key="ticket-blocker-dialog">
      <DialogSurface
        label={`Add Blocker to ${state.source.slug}`}
        preferredWidth={remify(544)}
        viewportGutter={remify(16)}
        rootAttributes={{
          'data-key': 'ticket-blocker-dialog',
          ...INSPECTOR_AND_EDITOR_TARGETS.blockerDialog.attrs,
          'data-controlled-open': 'true',
        }}
      >
        <form {...INSPECTOR_AND_EDITOR_ACTIONS.submitBlocker.attrs}>
          <List gap="m">
            <TicketReferencePicker
              fieldName={INSPECTOR_AND_EDITOR_TARGETS.blockerSearchField.value}
              query={state.query}
              candidates={candidates}
              selected={state.selected}
              searching={state.searching}
              selectAction={INSPECTOR_AND_EDITOR_ACTIONS.selectBlocker.value}
              clearAction={INSPECTOR_AND_EDITOR_ACTIONS.clearBlocker.value}
              candidateIcon={<LucideIcon icon={Ban} name="ban" />}
            />
            <div class="ticket-blocker-dialog__error" role="alert">
              {(state.error || (state.selected && validation)) && (
                <Text size="compact" tone="danger">
                  {state.error || validation}
                </Text>
              )}
            </div>
            {/* The close dialog's footer layout: Cancel leads, the decision trails (HS2-0DHVGK). */}
            <Row vAlign="middle" gap="xs">
              <Button
                type="button"
                action={INSPECTOR_AND_EDITOR_ACTIONS.cancelBlocker.value}
                emphasis="secondary"
                disabled={state.submitting}
              >
                Cancel
              </Button>
              <Spacer flex />
              <Button type="submit" tone="brand" disabled={Boolean(validation) || state.submitting}>
                {state.submitting ? 'Adding…' : 'Add Blocker'}
              </Button>
            </Row>
          </List>
        </form>
      </DialogSurface>
    </div>
  );
}
