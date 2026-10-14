import { describe, expect, it } from 'vitest';

import type { TicketRow as WireTicketRow } from '../api';
import { railDetailSource } from './terminal-rail-surface';

const row = (slug: string) => ({ slug }) as WireTicketRow;

describe('railDetailSource (HS2-1ABEYX)', () => {
  it('pushes nothing on the root screen or without exactly one selection', () => {
    expect(railDetailSource('root', ['A'], 'A', [row('A')])).toBeUndefined();
    expect(railDetailSource('ticket', [], undefined, [])).toBeUndefined();
    expect(railDetailSource('ticket', ['A', 'B'], 'A', [row('A'), row('B')])).toBeUndefined();
  });

  it('pushes the row-backed partial while an uncached ticket loads, then the loaded detail', () => {
    const rows = [row('A')];
    expect(railDetailSource('ticket', ['A'], undefined, rows)).toEqual({ kind: 'preview', row: rows[0] });
    expect(railDetailSource('ticket', ['A'], 'A', rows)).toEqual({ kind: 'loaded' });
  });

  it('previews the new row instead of the previous loaded ticket when the selection switches', () => {
    const rows = [row('B')];
    expect(railDetailSource('ticket', ['B'], 'A', rows)).toEqual({ kind: 'preview', row: rows[0] });
  });

  it('pushes nothing when the selected row is not available', () => {
    expect(railDetailSource('ticket', ['A'], undefined, [])).toBeUndefined();
  });
});
