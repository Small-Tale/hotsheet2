import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import './ticket-link-choice-dialog.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ArrowRight, CircleDot, GitBranch } from 'lucide';

import {
  NOTIFICATIONS_AND_LINKS_ACTIONS,
  NOTIFICATIONS_AND_LINKS_TARGETS,
} from '../interaction-attrs/notifications-and-links';
import { ticketLinkMatchKey, type TicketLinkResolution } from '../ticket-link-resolution';

export type TicketLinkChoice = Extract<TicketLinkResolution, { kind: 'choose' }>;

export function TicketLinkChoiceDialog({ choice }: { choice?: TicketLinkChoice }) {
  if (!choice) return <></>;
  return (
    <wa-dialog
      class="ticket-link-choice-dialog"
      {...NOTIFICATIONS_AND_LINKS_TARGETS.ticketLinkChoiceDialog.attrs}
      label={`Choose ${choice.reference.slug}`}
      open
      with-footer
    >
      <div class="ticket-link-choice-dialog__body">
        <p>More than one ticket has this exact reference. Choose the source you meant.</p>
        <ul class="ticket-link-choice-dialog__matches" aria-label={`Exact matches for ${choice.reference.raw}`}>
          {choice.matches.map((match) => (
            <li>
              <button
                type="button"
                {...NOTIFICATIONS_AND_LINKS_ACTIONS.selectTicketLinkMatch.attrs}
                data-match-key={ticketLinkMatchKey(match)}
                data-ticket-project-id={match.projectId}
                data-ticket-qualified-id={match.qualifiedId}
              >
                <span class="ticket-link-choice-dialog__status" data-status={match.status}>
                  <LucideIcon icon={CircleDot} name="circle-dot" />
                </span>
                <span class="ticket-link-choice-dialog__ticket">
                  <strong>{match.slug}</strong>
                  <span>{match.title}</span>
                </span>
                <span class="ticket-link-choice-dialog__source">
                  <span>
                    <LucideIcon icon={GitBranch} name="git-branch" />
                    {match.connectionId}
                  </span>
                  <small>{match.projectName}</small>
                </span>
                <LucideIcon icon={ArrowRight} name="arrow-right" />
              </button>
            </li>
          ))}
        </ul>
      </div>
      <div slot="footer" class="ticket-link-choice-dialog__actions">
        <wa-button
          type="button"
          appearance="outlined"
          {...NOTIFICATIONS_AND_LINKS_ACTIONS.cancelTicketLinkChoice.attrs}
        >
          Cancel
        </wa-button>
      </div>
    </wa-dialog>
  );
}
