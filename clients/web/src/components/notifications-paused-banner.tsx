import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { BellOff } from 'lucide';

import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import { notificationsPausedDetail } from '../notification-pause';
import { ShellBanner } from './shell-banner';

/**
 * Shown in every project while notifications are paused (HS2-QYA9SC): says how many permission
 * requests are waiting and resumes popups with one click. It is a shared shell strip (HS2-DNDW36).
 */
export function NotificationsPausedBanner({ waiting, halted = 0 }: { waiting: number; halted?: number }) {
  return (
    <ShellBanner
      component="notifications-paused-banner"
      title="Notifications paused"
      detail={
        notificationsPausedDetail(waiting) +
        (halted > 0 ? ` ${halted} halted ${halted === 1 ? 'session waits' : 'sessions wait'} until Resume.` : '')
      }
      tone="info"
      icon={<LucideIcon icon={BellOff} name="bell-off" />}
      action={
        <button type="button" {...NOTIFICATIONS_AND_LINKS_ACTIONS.resumeNotifications.attrs}>
          Resume
        </button>
      }
    />
  );
}
