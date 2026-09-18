import { describe, expect, it } from 'vitest';

import { boardColumnHasMore, type BoardColumnPage,boardColumnStatus, isPerColumnBoardView } from './board-pagination';

describe('board pagination', () => {
  it('maps each status column to its wire status', () => {
    expect(boardColumnStatus('not-started')).toBe('not_started');
    expect(boardColumnStatus('started')).toBe('started');
    expect(boardColumnStatus('completed')).toBe('completed');
    expect(boardColumnStatus('verified')).toBe('verified');
    // Single-collection board columns are not status-scoped (they paginate globally).
    expect(boardColumnStatus('backlog')).toBeUndefined();
    expect(boardColumnStatus('archive')).toBeUndefined();
    expect(boardColumnStatus('trash')).toBeUndefined();
  });

  it('only paginates per column for the multi-status board views', () => {
    expect(isPerColumnBoardView('all', false)).toBe(true);
    // Search results, single-collection views, and the errors view keep the global cursor.
    expect(isPerColumnBoardView('all', true)).toBe(false);
    expect(isPerColumnBoardView('backlog', false)).toBe(false);
    expect(isPerColumnBoardView('archive', false)).toBe(false);
    expect(isPerColumnBoardView('trash', false)).toBe(false);
    expect(isPerColumnBoardView('errors', false)).toBe(false);
  });

  it('reports more rows only while the authoritative total exceeds the loaded rows', () => {
    // Short column with 2 of 14 loaded and no independent page yet — the bug case: it must offer more.
    expect(boardColumnHasMore(14, 2, undefined)).toBe(true);
    // Fully loaded.
    expect(boardColumnHasMore(14, 14, { cursor: undefined, loaded: 14 })).toBe(false);
    // A page that still carries a cursor keeps loading.
    expect(boardColumnHasMore(500, 200, { cursor: 'after-200', loaded: 200 })).toBe(true);
    // Server-confirmed exhaustion wins even if a stale total still looks larger.
    expect(boardColumnHasMore(15, 14, { cursor: undefined, loaded: 14, exhausted: true } satisfies BoardColumnPage)).toBe(false);
    // Empty column never offers more.
    expect(boardColumnHasMore(0, 0, undefined)).toBe(false);
  });
});
