import './notifications-paused-banner.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { BellOff } from 'lucide';

import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import { notificationsPausedDetail } from '../notification-pause';

/**
 * Shown in every project while notifications are paused (HS2-QYA9SC): says how many permission
 * requests are waiting and resumes popups with one click.
 */
export function NotificationsPausedBanner({ waiting, halted = 0 }: { waiting: number; halted?: number }) {
  return (
    <div class="notifications-paused-banner" data-component="notifications-paused-banner">
      <StateBanner
        title="Notifications paused"
        detail={
          notificationsPausedDetail(waiting) +
          (halted > 0 ? ` ${halted} halted ${halted === 1 ? 'session waits' : 'sessions wait'} until Resume.` : '')
        }
        tone="info"
        urgency="status"
        copyLayout="stacked"
        icon={<LucideIcon icon={BellOff} name="bell-off" />}
        action={
          <button type="button" {...NOTIFICATIONS_AND_LINKS_ACTIONS.resumeNotifications.attrs}>
            Resume
          </button>
        }
      />
    </div>
  );
}
