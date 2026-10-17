import '@kerfjs/ui/layout.css';

import { Badge } from '@kerfjs/ui/badge';
import { Button } from '@kerfjs/ui/button';
import { rem } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Bell, BellOff, CalendarDays, Clock3 } from 'lucide';

import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import { SidebarPane, type SidebarPanelParts } from './sidebar-panel';

export type NotificationView = 'pending' | 'day' | 'week';

const views = [
  { id: 'pending', label: 'Pending', icon: Bell, iconName: 'bell' },
  { id: 'day', label: 'Last 24 Hours', icon: Clock3, iconName: 'clock-3' },
  { id: 'week', label: 'Last 7 Days', icon: CalendarDays, iconName: 'calendar-days' },
] as const;

export function notificationViewTitle(view: NotificationView): string {
  return views.find((item) => item.id === view)?.label ?? 'Notifications';
}

/** The notification navigator's panel parts for the Workbench's left rail (HS2-RWGQWN). */
export function notificationNavigationPanel({
  selected,
  counts,
  paused = false,
}: {
  selected: NotificationView;
  counts: Record<NotificationView, number>;
  /** The app-wide notification pause; the footer toggles it (HS2-QYA9SC). */
  paused?: boolean;
}): SidebarPanelParts {
  const content = (
    <List controlInsets="tb">
      <section>
        <ListHeader label="Notifications" />
        <List gap={rem(0.125)}>
          {views.map((item) => (
            <ListItem
              action="select-notification-view"
              itemId={item.id}
              selected={selected === item.id}
              icon={<LucideIcon icon={item.icon} name={item.iconName} />}
              label={item.label}
              trailing={
                <Badge
                  appearance="quiet"
                  size="compact"
                  tone={item.id === 'pending' && counts.pending > 0 ? 'danger' : 'neutral'}
                  className="notification-navigation__count"
                >
                  {String(counts[item.id])}
                </Badge>
              }
            />
          ))}
        </List>
      </section>
    </List>
  );
  // Kerf's Pane owns its footer's safe-area edges, so the inset lives on this inner box (HS2-KMDJRH).
  const footer = (
    <List controlInsets="trbl" rootAttributes={{ 'data-paused': String(paused) }}>
      <Button
        size="compact"
        emphasis="secondary"
        action={
          paused
            ? NOTIFICATIONS_AND_LINKS_ACTIONS.resumeNotifications.value
            : NOTIFICATIONS_AND_LINKS_ACTIONS.pauseNotifications.value
        }
        title={
          paused
            ? 'Show permission popups again in every project'
            : 'Stop permission popups in every project until you resume'
        }
        icon={<LucideIcon size="s" icon={paused ? Bell : BellOff} name={paused ? 'bell' : 'bell-off'} />}
      >
        {paused ? 'Resume Notifications' : 'Pause Notifications'}
      </Button>
    </List>
  );
  return {
    label: 'Notification views',
    toolbar: { label: 'Notification sidebar toolbar', dividerSides: '' },
    toggle: { action: 'toggle-project-sidebar', name: 'Notification Sidebar' },
    content,
    footer,
    pane: { contentElement: 'nav', contentLabel: 'Notification views' },
  };
}

export function NotificationNavigation({
  selected,
  counts,
  paused = false,
  collapseControl = false,
}: {
  selected: NotificationView;
  counts: Record<NotificationView, number>;
  paused?: boolean;
  collapseControl?: boolean;
}) {
  return (
    <SidebarPane
      parts={notificationNavigationPanel({ selected, counts, paused })}
      className="notification-navigation"
      collapseControl={collapseControl}
    />
  );
}
