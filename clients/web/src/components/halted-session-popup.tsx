import './halted-session-popup.css';

import { foregroundColorVar } from '@kerfjs/ui/css-values';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Bot, CircleAlert } from 'lucide';

import type { HaltedSessionEpisode } from '../halted-sessions';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import {
  TOP_LAYER_DISMISS_ATTRIBUTE,
  TOP_LAYER_OVERLAY_ATTRIBUTE,
  TOP_LAYER_PRESENTATION_KEY_ATTRIBUTE,
} from '../top-layer-overlay';

/** A native top-layer shell around the same compact card pattern as permission popups. */
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
      <article class="halted-session-popup__card">
        <header class="halted-session-popup__header">
          <span class="halted-session-popup__identity">
            <LucideIcon icon={Bot} name="bot" size="s" color={foregroundColorVar('--wa-color-success-fill-loud')} />
            <strong>{episode.sessionName}</strong>
          </span>
          <span class="halted-session-popup__project" title={episode.projectName}>
            {episode.projectName}
          </span>
        </header>
        <div class="halted-session-popup__summary">
          <LucideIcon
            icon={CircleAlert}
            name="circle-alert"
            size={17.6}
            color={foregroundColorVar('--wa-color-danger-fill-loud')}
          />
          <strong>AI session halted</strong>
        </div>
        <pre class="halted-session-popup__details">
          <code>{episode.message}</code>
        </pre>
        <footer class="halted-session-popup__footer">
          <div class="halted-session-popup__quiet-actions">
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
          </div>
          <button
            class="halted-session-popup__primary"
            type="button"
            {...NOTIFICATIONS_AND_LINKS_ACTIONS.openHaltedSession.attrs}
            data-halt-key={episode.key}
          >
            Open session
          </button>
        </footer>
      </article>
    </dialog>
  );
}
