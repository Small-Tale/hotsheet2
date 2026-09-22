import '@kerfjs/ui/layout.css';
import './settings-navigation.css';

import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Pane } from '@kerfjs/ui/pane';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { Bell, CalendarDays, Clock3, PanelLeftClose } from 'lucide';

export type NotificationView = 'pending' | 'day' | 'week';

const views = [
  { id: 'pending', label: 'Pending', icon: Bell, iconName: 'bell' },
  { id: 'day', label: 'Last 24 Hours', icon: Clock3, iconName: 'clock-3' },
  { id: 'week', label: 'Last 7 Days', icon: CalendarDays, iconName: 'calendar-days' },
] as const;

export function notificationViewTitle(view: NotificationView): string {
  return views.find((item) => item.id === view)?.label ?? 'Notifications';
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
  const header = collapseControl ? (
    <Toolbar
      divider={false}
      trailing={
        <ToolbarControlGroup appearance="borderless" single>
          <button
            type="button"
            data-action="toggle-project-sidebar"
            aria-label="Hide notification sidebar"
            title="Hide notification sidebar"
          >
            <LucideIcon icon={PanelLeftClose} name="panel-left-close" />
          </button>
        </ToolbarControlGroup>
      }
    />
  ) : undefined;
  return (
    <Pane
      element="aside"
      label="Notification views"
      className="settings-navigation"
      header={header}
      contentClassName="settings-navigation__content"
    >
      <section>
        <ListHeader label="Notifications" />
        <nav aria-label="Notification views">
          {views.map((item) => (
            <ListItem
              action="select-notification-view"
              itemId={item.id}
              selected={selected === item.id}
              icon={<LucideIcon icon={item.icon} name={item.iconName} />}
              label={item.label}
              trailing={
                <small
                  class="kui-list-item__count"
                  data-attention={String(item.id === 'pending' && counts.pending > 0)}
                >
                  {counts[item.id]}
                </small>
              }
            />
          ))}
        </nav>
      </section>
    </Pane>
  );
}
