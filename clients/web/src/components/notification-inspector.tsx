import { List } from '@kerfjs/ui/list';

import { inspectorToggle, SidebarPane, type SidebarPanelParts } from './sidebar-panel';

/** The notifications view's (empty) inspector panel parts for the Workbench's right rail (HS2-QQW6CT). */
export function notificationInspectorPanel(): SidebarPanelParts {
  return {
    label: 'Notification inspector',
    toolbar: { label: 'Notification inspector toolbar', dividerSides: '' },
    toggle: inspectorToggle('notification inspector'),
    content: <List fill className="notification-inspector-empty" />,
    pane: {},
  };
}

export function NotificationInspector({ collapseControl = false }: { collapseControl?: boolean } = {}) {
  return <SidebarPane parts={notificationInspectorPanel()} side="right" collapseControl={collapseControl} />;
}
