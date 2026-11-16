import './ticket-follow-up-link-dialog.css';

import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Spacer } from '@kerfjs/ui/spacer';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { GitBranch } from 'lucide';

import { REPOSITORY_ACTIONS, REPOSITORY_TARGETS } from '../interaction-attrs/repository';
import { duplicateTargetKey } from '../ticket-close';
import { type FollowUpLinkTicket, isLinkedFollowUp, validateFollowUpLink } from '../ticket-follow-up-link';
import { TicketReferencePicker } from './ticket-reference-picker';

export interface TicketFollowUpLinkDialogState {
  /** The inspected ticket that records the follow-up relation. */
  source: FollowUpLinkTicket;
  /** Keys ({@link duplicateTargetKey}) of the follow-ups the source already records. */
  existing: readonly string[];
  query: string;
  candidates: readonly FollowUpLinkTicket[];
  selected?: FollowUpLinkTicket;
  searching?: boolean;
  submitting?: boolean;
  error?: string;
  /**
   * The reader frame that opened the dialog, which renders it inside that modal reader so it stays
   * interactive (HS2-NXSMC7); absent when the Inspector opened it at the app root.
   */
  readerScope?: string;
}

/**
 * Link an existing ticket as a follow-up of the inspected ticket (HS2-MCGCPQ). The Code Review
 * tab's Follow-Ups action opens it. The person searches every open project with the shared
 * {@link TicketReferencePicker}, and Link Follow-Up records the qualified relation. Linking a
 * ticket that is already a follow-up is allowed and changes nothing; the dialog says so first.
 */
export function TicketFollowUpLinkDialog({ state }: { state?: TicketFollowUpLinkDialogState }) {
  if (!state) return <></>;
  const validation = validateFollowUpLink(state.source, state.selected),
    already = isLinkedFollowUp(state.existing, state.selected),
    candidates = state.candidates.filter(
      (candidate) => duplicateTargetKey(candidate) !== duplicateTargetKey(state.source),
    );
  return (
    <div class="ticket-follow-up-link-dialog" data-key="ticket-follow-up-link-dialog">
      <DialogSurface
        label={`Link Follow-Up to ${state.source.slug}`}
        preferredWidth={remify(544)}
        viewportGutter={remify(16)}
        rootAttributes={{
          'data-key': 'ticket-follow-up-link-dialog',
          ...REPOSITORY_TARGETS.followUpLinkDialog.attrs,
          'data-controlled-open': 'true',
        }}
      >
        <form {...REPOSITORY_ACTIONS.submitFollowUpLink.attrs}>
          <List gap="m">
            <TicketReferencePicker
              fieldName="follow-up-link-target-search"
              query={state.query}
              candidates={candidates}
              selected={state.selected}
              searching={state.searching}
              selectAction={REPOSITORY_ACTIONS.selectFollowUpLinkTarget.value}
              clearAction={REPOSITORY_ACTIONS.clearFollowUpLinkTarget.value}
              candidateIcon={<LucideIcon icon={GitBranch} name="git-branch" />}
            />
            {already && state.selected && (
              <Text size="compact" tone="quiet" role="status">
                {`${state.selected.slug} is already a follow-up of ${state.source.slug}. Linking it again changes nothing.`}
              </Text>
            )}
            <div class="ticket-follow-up-link-dialog__error" role="alert">
              {(state.error || validation) && (
                <Text size="compact" tone="danger">
                  {state.error || validation}
                </Text>
              )}
            </div>
            {/* The close dialog's footer layout: Cancel leads, the decision trails (HS2-0DHVGK). */}
            <Row vAlign="middle" gap="xs">
              <Button
                type="button"
                action={REPOSITORY_ACTIONS.cancelFollowUpLink.value}
                emphasis="secondary"
                disabled={state.submitting}
              >
                Cancel
              </Button>
              <Spacer flex />
              <Button type="submit" tone="brand" disabled={Boolean(validation) || state.submitting}>
                {state.submitting ? 'Linking…' : 'Link Follow-Up'}
              </Button>
            </Row>
          </List>
        </form>
      </DialogSurface>
    </div>
  );
}
