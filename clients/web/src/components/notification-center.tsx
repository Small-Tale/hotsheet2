import './notification-center.css';

import { Bell } from 'lucide';

import type { PermissionHistoryItem, PermissionItem } from '../permission-notifications';
import { LucideIcon } from './lucide-icon';
import { PermissionRequestCard } from './permission-request-card';

export interface NotificationCenterProps {
  pending: PermissionItem[];
  history: PermissionHistoryItem[];
  countdowns?: Readonly<Record<string, string>>;
  countdownAction?: 'allow' | 'deny';
}

/** Global pending-permission queue and newest-first resolution history. */
export function NotificationCenter({ pending, history, countdowns = {}, countdownAction = 'allow' }: NotificationCenterProps) {
  return <section class="notification-center" data-component="notification-center" aria-labelledby="notification-center-title">
    <header class="notification-center__header">
      <span><LucideIcon icon={Bell} name="bell" /><h1 id="notification-center-title">Notifications</h1></span>
      {pending.length > 0 && <strong aria-label={`${pending.length} pending notification${pending.length === 1 ? '' : 's'}`}>{pending.length}</strong>}
    </header>
    <section class="notification-center__section" aria-labelledby="pending-notifications-title">
      <h2 id="pending-notifications-title">Pending <span>{pending.length}</span></h2>
      {pending.length > 0 ? <div class="notification-center__items">{pending.map(item => <PermissionRequestCard item={item} countdown={countdowns[item.key]} countdownAction={countdownAction} />)}</div> : <p class="notification-center__empty">No requests need your attention.</p>}
    </section>
    <section class="notification-center__section" aria-labelledby="previous-notifications-title">
      <h2 id="previous-notifications-title">Previous</h2>
      {history.length > 0 ? <div class="notification-center__items">{history.map(item => <PermissionRequestCard item={item} />)}</div> : <p class="notification-center__empty">Previous decisions will appear here.</p>}
    </section>
  </section>;
}
