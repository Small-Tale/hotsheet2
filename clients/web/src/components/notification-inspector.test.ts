import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { NotificationInspector, notificationInspectorPanel } from './notification-inspector';

describe('NotificationInspector', () => {
  it('owns the empty inspector close control and its presentation', () => {
    const markup = String(NotificationInspector({ collapseControl: true }));
    const css = readFileSync(resolve(import.meta.dirname, 'notification-inspector.css'), 'utf8');
    expect(markup).toContain('aria-label="Notification inspector"');
    expect(markup).toContain('data-action="toggle-ticket-inspector"');
    expect(markup).toContain('aria-label="Hide notification inspector"');
    expect(markup).toContain('data-lucide="panel-right-close"');
    // Inside the Workbench the rail renders the standard toggle itself (HS2-QQW6CT).
    expect(String(NotificationInspector())).not.toContain('toggle-ticket-inspector');
    expect(notificationInspectorPanel().toggle).toEqual({
      action: 'toggle-ticket-inspector',
      name: 'notification inspector',
    });
    expect(css).toContain('.notification-inspector-empty');
  });
});
