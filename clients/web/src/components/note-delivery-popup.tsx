import './note-delivery-popup.css';

import { Button } from '@kerfjs/ui/button';
import { foregroundColorVar } from '@kerfjs/ui/css-values';
import { MessageSquareShare } from 'lucide';

import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import {
  TOP_LAYER_DISMISS_ATTRIBUTE,
  TOP_LAYER_OVERLAY_ATTRIBUTE,
  TOP_LAYER_PRESENTATION_KEY_ATTRIBUTE,
} from '../top-layer-overlay';
import { NotificationCard } from './notification-card';

/** A saved human note waiting for the person to pick, or start, the AI session to notify. */
export interface NoteDeliveryPrompt {
  /** Unique per saved note, so a new note presents again. */
  key: string;
  /** The ticket's qualified id the notify request names. */
  ticketId: string;
  slug: string;
  title: string;
  projectName: string;
  kind: 'choose' | 'start';
  /** Each candidate session with its human label (HS2-5MCAKY). */
  sessions: ReadonlyArray<{ id: string; label: string; description: string }>;
}

/**
 * The Choose an AI Session prompt at app level (HS2-6RXBS9): after a human note several sessions could
 * receive, or none is running, the same native top-layer card as a permission request or a halted
 * session asks which session to notify, whatever project, ticket, or view is open.
 */
export function NoteDeliveryPopup({ prompt }: { prompt: NoteDeliveryPrompt }) {
  const choose = prompt.kind === 'choose';
  return (
    <dialog
      class="note-delivery-popup"
      data-component="note-delivery-popup"
      data-kind={prompt.kind}
      popover="manual"
      {...{ [TOP_LAYER_OVERLAY_ATTRIBUTE]: '' }}
      {...{ [TOP_LAYER_PRESENTATION_KEY_ATTRIBUTE]: prompt.key }}
      tabindex={-1}
      aria-label={choose ? 'Choose an AI Session' : 'Start an AI Session'}
    >
      <NotificationCard
        elevation="popup"
        // One button per session can outgrow a phone-width footer, so the choices stack.
        footerLayout={choose ? 'stack' : 'wrap'}
        identity={prompt.slug}
        projectName={prompt.projectName}
        summary={{
          icon: MessageSquareShare,
          iconName: 'message-square-share',
          label: choose ? 'Choose an AI Session' : 'Start an AI Session',
          color: foregroundColorVar('--wa-color-brand-fill-loud'),
        }}
        details={{
          text: choose
            ? `Your note on “${prompt.title}” was saved. Select the session to notify.`
            : `Your note on “${prompt.title}” was saved. Start a session in this project to continue work on the ticket.`,
          appearance: 'prose',
        }}
        quietActions={
          <Button
            emphasis="plain"
            size="compact"
            action={NOTIFICATIONS_AND_LINKS_ACTIONS.dismissFeedbackDelivery.value}
            rootAttributes={{ ...{ [TOP_LAYER_DISMISS_ATTRIBUTE]: '' } }}
          >
            Dismiss
          </Button>
        }
        endActions={
          choose ? (
            <>
              {prompt.sessions.map((session) => (
                <Button
                  size="compact"
                  emphasis="secondary"
                  action={NOTIFICATIONS_AND_LINKS_ACTIONS.chooseFeedbackSession.value}
                  title={session.description}
                  rootAttributes={{ 'data-ticket-id': prompt.ticketId, 'data-connection-id': session.id }}
                >
                  {session.label}
                </Button>
              ))}
            </>
          ) : (
            <Button
              size="compact"
              emphasis="primary"
              tone="brand"
              action={NOTIFICATIONS_AND_LINKS_ACTIONS.startFeedbackSession.value}
            >
              Start AI Session
            </Button>
          )
        }
      />
    </dialog>
  );
}
