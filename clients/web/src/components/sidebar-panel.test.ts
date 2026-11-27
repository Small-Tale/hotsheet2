import { describe, expect, it } from 'vitest';

import { notificationInspectorPanel } from './notification-inspector';
import { SidebarPane, type SidebarPaneRootClass } from './sidebar-panel';

const rootClass = (markup: string) => /^<[a-z]+ class="([^"]*)"/.exec(markup)?.[1];

describe('SidebarPane', () => {
  it('HS2-5FFT4J forwards the literal-union root class to the Kerf Pane, or none without one', () => {
    const parts = notificationInspectorPanel();
    expect(rootClass(String(SidebarPane({ parts })))).toBe('kui-pane');
    const classes: SidebarPaneRootClass[] = [
      'project-sidebar',
      'settings-navigation',
      'terminal-operations-sidebar',
      'notification-navigation',
    ];
    for (const className of classes)
      expect(rootClass(String(SidebarPane({ parts, className })))).toBe(`kui-pane ${className}`);
  });
});
