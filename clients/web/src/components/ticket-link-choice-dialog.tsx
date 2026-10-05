import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';

import { uiColor } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
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
    <DialogSurface bodyInset="comfortable" footerInset="comfortable">
      <wa-dialog
        {...NOTIFICATIONS_AND_LINKS_TARGETS.ticketLinkChoiceDialog.attrs}
        label={`Choose ${choice.reference.slug}`}
        open
        with-footer
      >
        <List gap="m">
          <Text flush data-ticket-choice-intro>
            More than one ticket has this exact reference. Choose the source you meant.
          </Text>
          <nav aria-label={`Exact matches for ${choice.reference.raw}`}>
            <List gap="none" rootAttributes={{ 'data-ticket-matches': 'true' }}>
              {choice.matches.map((match) => (
                <ListItem
                  action={NOTIFICATIONS_AND_LINKS_ACTIONS.selectTicketLinkMatch.attrs['data-action']}
                  itemId={ticketLinkMatchKey(match)}
                  rootAttributes={{
                    'data-match-key': ticketLinkMatchKey(match),
                    'data-ticket-project-id': match.projectId,
                    'data-ticket-qualified-id': match.qualifiedId,
                    'data-status': match.status,
                  }}
                  label={match.slug}
                  description={
                    <Text variant="span" wrap="anywhere">
                      {match.title}
                      <br />
                      <Text variant="span" size="compact" tone="quiet" wrap="anywhere">
                        <LucideIcon inline size="xs" icon={GitBranch} name="git-branch" /> {match.connectionId} ·{' '}
                        {match.projectName}
                      </Text>
                    </Text>
                  }
                  multiline
                  density="spacious"
                  icon={
                    <LucideIcon
                      size={13.6}
                      icon={CircleDot}
                      name="circle-dot"
                      color={
                        match.status === 'started'
                          ? uiColor('brand-on-quiet')
                          : match.status === 'completed' || match.status === 'verified'
                            ? uiColor('success-on-quiet')
                            : uiColor('neutral-on-quiet')
                      }
                    />
                  }
                  trailing={
                    <LucideIcon icon={ArrowRight} name="arrow-right" size="s" color={uiColor('neutral-on-quiet')} />
                  }
                />
              ))}
            </List>
          </nav>
        </List>
        <Row slot="footer" hAlign="right" vAlign="middle" gap="xs">
          <wa-button
            type="button"
            appearance="outlined"
            {...NOTIFICATIONS_AND_LINKS_ACTIONS.cancelTicketLinkChoice.attrs}
          >
            Cancel
          </wa-button>
        </Row>
      </wa-dialog>
    </DialogSurface>
  );
}
