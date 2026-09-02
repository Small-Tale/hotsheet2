import { describe, expect, it } from 'vitest';

import { TicketInspectorPlaceholder } from './ticket-inspector-placeholder';

describe('TicketInspectorPlaceholder', () => {
  it('omits the toolbar separator only when the inspector is empty', () => {
    const empty = String(TicketInspectorPlaceholder({ selectionCount: 0 }));
    expect(empty).toContain('data-component="toolbar" data-divider="false"');
    expect(empty).toContain('Select a ticket to see and edit its details');

    const loading = String(TicketInspectorPlaceholder({ selectionCount: 1 }));
    expect(loading).toContain('data-component="toolbar" data-divider="true"');

    const multi = String(TicketInspectorPlaceholder({ selectionCount: 2 }));
    expect(multi).toContain('data-component="toolbar" data-divider="true"');
    expect(multi).toContain('2 items selected — use batch actions to edit them together');
  });
});
