import { describe, expect, it } from 'vitest';

import { TicketInspectorPanel, type TicketInspectorPanelRootClass } from './ticket-inspector-panel';

const rootClass = (markup: string) => /^<[a-z]+ class="([^"]*)"/.exec(markup)?.[1];

describe('TicketInspectorPanel', () => {
  it('HS2-5FFT4J composes the literal-union root class exactly for each presentation and panel class', () => {
    expect(rootClass(String(TicketInspectorPanel({ component: 'x' })))).toBe('ticket-inspector-panel');
    expect(rootClass(String(TicketInspectorPanel({ component: 'x', presentation: 'reader' })))).toBe(
      'ticket-inspector-panel ticket-inspector-panel--reader',
    );
    const panels: TicketInspectorPanelRootClass[] = [
      'ticket-info-panel',
      'ticket-timeline',
      'ticket-code-review ticket-code-review--panel',
      'ticket-attachments',
    ];
    for (const className of panels) {
      expect(rootClass(String(TicketInspectorPanel({ component: 'x', className })))).toBe(
        `ticket-inspector-panel ${className}`,
      );
      expect(rootClass(String(TicketInspectorPanel({ component: 'x', presentation: 'reader', className })))).toBe(
        `ticket-inspector-panel ticket-inspector-panel--reader ${className}`,
      );
    }
  });
});
