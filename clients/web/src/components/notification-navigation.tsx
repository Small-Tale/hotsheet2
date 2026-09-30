import '@kerfjs/ui/layout.css';
import './settings-navigation.css';

import { rem } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Bell, CalendarDays, Clock3 } from 'lucide';

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
}: {
  selected: NotificationView;
  counts: Record<NotificationView, number>;
}): SidebarPanelParts {
  const content = (
    <div class="settings-navigation__content">
      <section>
        <ListHeader label="Notifications" />
        <div>
          <List gap={rem(0.125)}>
            {views.map((item) => (
              <ListItem
                action="select-notification-view"
                itemId={item.id}
                selected={selected === item.id}
                icon={<LucideIcon icon={item.icon} name={item.iconName} />}
                label={item.label}
                trailing={
                  <small
                    class="notification-navigation__count"
                    data-attention={String(item.id === 'pending' && counts.pending > 0)}
                  >
                    {counts[item.id]}
                  </small>
                }
              />
            ))}
          </List>
        </div>
      </section>
    </div>
  );
  return {
    label: 'Notification views',
    toolbar: { label: 'Notification sidebar toolbar', dividerSides: '' },
    toggle: { action: 'toggle-project-sidebar', name: 'notification sidebar' },
    content,
    pane: { contentElement: 'nav', contentLabel: 'Notification views' },
  };
}

export function NotificationNavigation({
  selected,
  counts,
  collapseControl = false,
}: {
  selected: NotificationView;
  counts: Record<NotificationView, number>;
  collapseControl?: boolean;
}) {
  return (
    <SidebarPane
      parts={notificationNavigationPanel({ selected, counts })}
      className="settings-navigation"
      collapseControl={collapseControl}
    />
  );
}
