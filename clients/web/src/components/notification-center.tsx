import './notification-center.css';

import type { PermissionHistoryItem, PermissionItem } from '../permission-notifications';
import { PermissionRequestCard } from './permission-request-card';

export interface NotificationCenterProps {
  pending: PermissionItem[];
  history: PermissionHistoryItem[];
  countdowns?: Readonly<Record<string, string>>;
  countdownAction?: 'allow' | 'deny';
  title?: string;
}

/** Global pending-permission queue and newest-first resolution history. */
export function NotificationCenter({ pending, history, countdowns = {}, countdownAction = 'allow', title = 'Notifications' }: NotificationCenterProps) {
  const empty = pending.length === 0 && history.length === 0;
  return <section class="notification-center" data-component="notification-center" aria-label={title}>
    {pending.length > 0 && <div class="notification-center__items">{pending.map(item => <PermissionRequestCard item={item} countdown={countdowns[item.key]} countdownAction={countdownAction} />)}</div>}
    {history.length > 0 && <div class="notification-center__items">{history.map(item => <PermissionRequestCard item={item} />)}</div>}
    {empty && <p class="notification-center__empty">{title === 'Pending' ? 'No requests need your attention.' : `No notification history in ${title.toLowerCase()}.`}</p>}
  </section>;
}
