import './settings-navigation.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { Bell, CalendarDays, Clock3, PanelLeftClose } from 'lucide';

import { MenuItem } from './menu-item';

export type NotificationView = 'pending' | 'day' | 'week';

const views = [
  { id: 'pending', label: 'Pending', icon: Bell, iconName: 'bell' },
  { id: 'day', label: 'Last 24 Hours', icon: Clock3, iconName: 'clock-3' },
  { id: 'week', label: 'Last 7 Days', icon: CalendarDays, iconName: 'calendar-days' },
] as const;

export function notificationViewTitle(view: NotificationView): string {
  return views.find(item => item.id === view)?.label ?? 'Notifications';
}

export function NotificationNavigation({ selected, counts, collapseControl = false }: { selected: NotificationView; counts: Record<NotificationView, number>; collapseControl?: boolean }) {
  return <aside class="settings-navigation" data-component="notification-navigation" aria-label="Notification views">
    {collapseControl && <Toolbar divider={false} trailing={<ToolbarControlGroup appearance="borderless" single><button type="button" data-action="toggle-project-sidebar" aria-label="Hide notification sidebar" title="Hide notification sidebar"><LucideIcon icon={PanelLeftClose} name="panel-left-close" /></button></ToolbarControlGroup>} />}
    <div class="settings-navigation__content">
      <p class="settings-navigation__heading">Notifications</p>
      <nav aria-label="Notification views">{views.map(item => <MenuItem action="select-notification-view" itemId={item.id} selected={selected === item.id} icon={<LucideIcon icon={item.icon} name={item.iconName} />} label={item.label} trailing={<small class="kui-menu-item__count" data-attention={String(item.id === 'pending' && counts.pending > 0)}>{counts[item.id]}</small>} />)}</nav>
    </div>
  </aside>;
}
