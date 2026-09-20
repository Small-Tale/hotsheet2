import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { NotificationInspector } from './notification-inspector';

describe('NotificationInspector', () => {
  it('owns the empty inspector close control and its presentation', () => {
    const markup = String(NotificationInspector());
    const css = readFileSync(resolve(import.meta.dirname, 'notification-inspector.css'), 'utf8');
    expect(markup).toContain('aria-label="Notification inspector"');
    expect(markup).toContain('data-action="close-ticket-inspector"');
    expect(css).toContain('.notification-inspector-empty');
  });
});
