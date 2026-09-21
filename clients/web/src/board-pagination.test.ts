import { describe, expect, it } from 'vitest';

import {
  applyBoardColumnFetch,
  boardColumnHasMore,
  type BoardColumnPage,
  boardColumnStatus,
  boardColumnStatuses,
  isPerColumnBoardView,
  nextBoardColumnFetch,
} from './board-pagination';

describe('board pagination', () => {
  it('maps each status column to its primary wire status', () => {
    expect(boardColumnStatus('not-started')).toBe('not_started');
    expect(boardColumnStatus('started')).toBe('started');
    expect(boardColumnStatus('completed')).toBe('completed');
    expect(boardColumnStatus('verified')).toBe('verified');
    // Single-collection board columns are not status-scoped (they paginate globally).
    expect(boardColumnStatus('backlog')).toBeUndefined();
    expect(boardColumnStatus('archive')).toBeUndefined();
    expect(boardColumnStatus('trash')).toBeUndefined();
  });

  it('lists each column ordered statuses, merging Verified into Completed when hidden (HS2-F2N4ZN)', () => {
    expect(boardColumnStatuses('not-started', false)).toEqual(['not_started']);
    expect(boardColumnStatuses('started', false)).toEqual(['started']);
    // Standalone Verified column: Completed owns only completed, Verified owns verified.
    expect(boardColumnStatuses('completed', false)).toEqual(['completed']);
    expect(boardColumnStatuses('verified', false)).toEqual(['verified']);
    // Hide-Verified merges: Completed pages completed then verified; no standalone Verified column.
    expect(boardColumnStatuses('completed', true)).toEqual(['completed', 'verified']);
    expect(boardColumnStatuses('verified', true)).toEqual([]);
    expect(boardColumnStatuses('backlog', false)).toEqual([]);
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
    expect(boardColumnHasMore(14, 14, { loaded: 14 })).toBe(false);
    // Not yet exhausted and the total still exceeds the loaded rows — keep loading.
    expect(boardColumnHasMore(500, 200, { loaded: 200 })).toBe(true);
    // Server-confirmed exhaustion wins even if a stale total still looks larger.
    expect(boardColumnHasMore(15, 14, { loaded: 14, exhausted: true } satisfies BoardColumnPage)).toBe(false);
    // Empty column never offers more.
    expect(boardColumnHasMore(0, 0, undefined)).toBe(false);
  });

  describe('multi-status column stream walk (HS2-F2N4ZN)', () => {
    it('picks the first non-exhausted status, continuing or starting its stream', () => {
      const statuses = ['completed', 'verified'];
      // No page yet → start the first status from the beginning.
      expect(nextBoardColumnFetch(statuses, undefined)).toEqual({ status: 'completed', cursor: undefined });
      // First status mid-stream → continue its cursor.
      expect(nextBoardColumnFetch(statuses, { loaded: 100, streams: { completed: { cursor: 'c1' } } })).toEqual({
        status: 'completed',
        cursor: 'c1',
      });
      // First status exhausted, second not started → start the second.
      expect(nextBoardColumnFetch(statuses, { loaded: 120, streams: { completed: { exhausted: true } } })).toEqual({
        status: 'verified',
        cursor: undefined,
      });
      // First exhausted, second mid-stream → continue the second.
      expect(
        nextBoardColumnFetch(statuses, {
          loaded: 150,
          streams: { completed: { exhausted: true }, verified: { cursor: 'v1' } },
        }),
      ).toEqual({ status: 'verified', cursor: 'v1' });
      // Every status exhausted → nothing left to fetch.
      expect(
        nextBoardColumnFetch(statuses, {
          loaded: 150,
          exhausted: true,
          streams: { completed: { exhausted: true }, verified: { exhausted: true } },
        }),
      ).toBeUndefined();
    });

    it('folds a fetch into the column page and only exhausts the column when every status is done', () => {
      const statuses = ['completed', 'verified'];
      // Fetch completed with more to come.
      const afterCompletedPage1 = applyBoardColumnFetch(statuses, undefined, 'completed', 'c1', 100);
      expect(afterCompletedPage1).toEqual({
        loaded: 100,
        exhausted: false,
        streams: { completed: { cursor: 'c1', exhausted: false } },
      });
      // Completed exhausts, but verified is untouched → column not yet exhausted.
      const afterCompletedDone = applyBoardColumnFetch(statuses, afterCompletedPage1, 'completed', undefined, 120);
      expect(afterCompletedDone).toEqual({
        loaded: 120,
        exhausted: false,
        streams: { completed: { cursor: undefined, exhausted: true } },
      });
      // Verified exhausts → the whole column is exhausted.
      const afterVerifiedDone = applyBoardColumnFetch(statuses, afterCompletedDone, 'verified', undefined, 150);
      expect(afterVerifiedDone).toEqual({
        loaded: 150,
        exhausted: true,
        streams: {
          completed: { cursor: undefined, exhausted: true },
          verified: { cursor: undefined, exhausted: true },
        },
      });
    });

    it('drives a full completed→verified walk with next/apply until exhausted', () => {
      const statuses = ['completed', 'verified'];
      let page: BoardColumnPage | undefined;
      // Step 1: start completed, more remains.
      let target = nextBoardColumnFetch(statuses, page);
      expect(target).toEqual({ status: 'completed', cursor: undefined });
      page = applyBoardColumnFetch(statuses, page, target!.status, 'c1', 100);
      expect(boardColumnHasMore(150, 100, page)).toBe(true);
      // Step 2: finish completed.
      target = nextBoardColumnFetch(statuses, page);
      expect(target).toEqual({ status: 'completed', cursor: 'c1' });
      page = applyBoardColumnFetch(statuses, page, target!.status, undefined, 120);
      // Completed is done but verified rows remain — the column must still offer more (the bug).
      expect(page.exhausted).toBe(false);
      expect(boardColumnHasMore(150, 120, page)).toBe(true);
      // Step 3: walk into verified and finish it.
      target = nextBoardColumnFetch(statuses, page);
      expect(target).toEqual({ status: 'verified', cursor: undefined });
      page = applyBoardColumnFetch(statuses, page, target!.status, undefined, 150);
      expect(page.exhausted).toBe(true);
      expect(boardColumnHasMore(150, 150, page)).toBe(false);
      // Step 4: nothing left to fetch.
      expect(nextBoardColumnFetch(statuses, page)).toBeUndefined();
    });

    it('exhausts a single-status column immediately when its one stream ends (adversarial: re-query after done)', () => {
      const statuses = ['not_started'];
      const page = applyBoardColumnFetch(statuses, undefined, 'not_started', undefined, 14);
      expect(page).toEqual({
        loaded: 14,
        exhausted: true,
        streams: { not_started: { cursor: undefined, exhausted: true } },
      });
      // A repeated fetch attempt after exhaustion finds nothing to do.
      expect(nextBoardColumnFetch(statuses, page)).toBeUndefined();
      expect(boardColumnHasMore(14, 14, page)).toBe(false);
    });
  });
});
