import { describe, expect, it } from 'vitest';

import { CorruptInspector, InspectorPlaceholder } from './ticket-inspector-surface';

describe('ticket inspector surfaces', () => {
  it('owns the selection-aware placeholder', () => {
    expect(String(InspectorPlaceholder({ selectionCount: 2 }))).toContain('2 items selected');
  });

  it('falls back to the same placeholder when a corrupt selection disappears', () => {
    expect(String(CorruptInspector({ selectionCount: 0 }))).toContain('Select a ticket');
  });
});
