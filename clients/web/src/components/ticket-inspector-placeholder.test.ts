import { describe, expect, it } from 'vitest';

import { TicketInspectorPlaceholder, ticketInspectorPlaceholderPanel } from './ticket-inspector-placeholder';

describe('TicketInspectorPlaceholder', () => {
  it('explains the empty and multi-selection states under the standard rail toggle (HS2-QQW6CT)', () => {
    const empty = String(TicketInspectorPlaceholder({ selectionCount: 0, collapseControl: true }));
    expect(empty).toContain('data-component="toolbar"');
    expect(empty).toContain('data-action="toggle-ticket-inspector"');
    expect(empty).toContain('aria-label="Hide ticket inspector"');
    expect(empty).toContain('Select a ticket to see and edit its details');

    // Without its collapse control (the Workbench renders the toggle) the standalone pane has no toolbar.
    expect(String(TicketInspectorPlaceholder({ selectionCount: 0 }))).not.toContain('data-component="toolbar"');

    const multi = String(TicketInspectorPlaceholder({ selectionCount: 2 }));
    expect(multi).toContain('2 items selected — use batch actions to edit them together');

    const parts = ticketInspectorPlaceholderPanel({ selectionCount: 1 });
    expect(parts.toggle).toEqual({ action: 'toggle-ticket-inspector', name: 'ticket inspector' });
    expect(String(parts.content)).toContain('data-ticket-inspector-placeholder="true"');
    expect(String(parts.content)).toContain('1 items selected');
  });
});
