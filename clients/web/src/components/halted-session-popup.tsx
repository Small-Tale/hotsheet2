import './halted-session-popup.css';

import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';

import type { HaltedSessionEpisode } from '../halted-sessions';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import {
  TOP_LAYER_DISMISS_ATTRIBUTE,
  TOP_LAYER_OVERLAY_ATTRIBUTE,
  TOP_LAYER_PRESENTATION_KEY_ATTRIBUTE,
} from '../top-layer-overlay';

/** Native top-layer geometry belongs to the app; all content layout uses public Kerf primitives. */
export function HaltedSessionPopup({ episode }: { episode: HaltedSessionEpisode }) {
  return (
    <dialog
      class="halted-session-popup"
      data-component="halted-session-popup"
      data-halt-key={episode.key}
      popover="manual"
      {...{ [TOP_LAYER_OVERLAY_ATTRIBUTE]: '' }}
      {...{ [TOP_LAYER_PRESENTATION_KEY_ATTRIBUTE]: episode.key }}
      tabindex={-1}
      aria-label="AI session halted"
    >
      <List gap="m" textInsets="trbl">
        <Text variant="h2" size="large" flush tone="danger">
          AI session halted
        </Text>
        <Text flush wrap="anywhere">
          {episode.projectName} · {episode.sessionName}
        </Text>
        <Text flush wrap="anywhere">
          {episode.message}
        </Text>
        <Row wrap gap="xs">
          <button
            type="button"
            {...NOTIFICATIONS_AND_LINKS_ACTIONS.openHaltedSession.attrs}
            data-halt-key={episode.key}
          >
            Open session
          </button>
          <button
            type="button"
            {...NOTIFICATIONS_AND_LINKS_ACTIONS.dismissHaltedSession.attrs}
            data-halt-key={episode.key}
            {...{ [TOP_LAYER_DISMISS_ATTRIBUTE]: '' }}
          >
            Dismiss
          </button>
          <button type="button" {...NOTIFICATIONS_AND_LINKS_ACTIONS.pauseNotifications.attrs}>
            Pause notifications
          </button>
        </Row>
      </List>
    </dialog>
  );
}
