import { describe, expect, it } from 'vitest';

import type { CorruptTicket } from '../api';
import {
  corruptInspectorPanel,
  inspectorPanel,
  inspectorPlaceholderPanel,
  inspectorSkeletonPanel,
  notificationInspectorSurfacePanel,
} from './ticket-inspector-surface';

describe('ticket inspector surfaces', () => {
  it('owns the selection-aware placeholder', () => {
    expect(String(inspectorPlaceholderPanel({ selectionCount: 2 }).content)).toContain('2 items selected');
  });

  it('falls back to the same placeholder when a corrupt selection disappears', () => {
    expect(String(corruptInspectorPanel({ selectionCount: 0 }).content)).toContain('Select a ticket');
  });

  it('routes a corrupt selection to its recovery panel', () => {
    const ticket = { store: 's', path: 'a/HS2-BAD.md', slug: 'HS2-BAD', error: 'bad' } as CorruptTicket;
    const parts = corruptInspectorPanel({ ticket, selectionCount: 1 });
    expect(String(parts.header)).toContain('HS2-BAD');
    expect(String(parts.content)).toContain('data-component="corrupt-ticket-inspector"');
  });

  it('gives every right-rail surface the one standard rail toggle (HS2-QQW6CT)', () => {
    const surfaces = [
      inspectorPanel({
        slug: 'HS2-ONE',
        title: 'One',
        status: 'started',
        priority: 'default',
        category: 'task',
        tags: [],
        details: '',
      }),
      inspectorPlaceholderPanel({ selectionCount: 0 }),
      inspectorSkeletonPanel({ slug: 'HS2-ONE' }),
      notificationInspectorSurfacePanel(),
      corruptInspectorPanel({ selectionCount: 0 }),
    ];
    for (const parts of surfaces) expect(parts.toggle.action).toBe('toggle-ticket-inspector');
    expect(surfaces.map((parts) => parts.toggle.name)).toEqual([
      'ticket inspector',
      'ticket inspector',
      'ticket inspector',
      'notification inspector',
      'ticket inspector',
    ]);
  });
});
