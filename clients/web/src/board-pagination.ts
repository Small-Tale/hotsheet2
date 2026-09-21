/** Per-column board pagination (HS2-8NBGBX).
 *
 * The board renders one column per ticket status. Pagination used to be global — a single checkout
 * cursor over the flat `tickets.value` list, bucketed into columns — so a long column (e.g. Completed)
 * consumed the page budget and short columns (e.g. Not Started, showing 2 of 14) could never load more,
 * and the one "Load more" continuation landed in whichever column happened to hold the last globally
 * loaded row. Each status column now paginates independently with its own status-filtered query and
 * cursor, while the loaded rows still live in the flat `tickets.value` union so selection, the
 * inspector, mutations, and drag/drop are unaffected. */

import type { TicketView } from './ticket-views';

/** The primary wire status of a board column, or undefined for a column that is not status-scoped
 * (single-collection board views paginate globally instead). This identifies *whether* a column
 * paginates per status; the ordered set of statuses it actually pages is `boardColumnStatuses`. */
export function boardColumnStatus(columnId: string): string | undefined {
  switch (columnId) {
    case 'not-started':
      return 'not_started';
    case 'started':
      return 'started';
    case 'completed':
      return 'completed';
    case 'verified':
      return 'verified';
    default:
      return undefined;
  }
}

/** The ordered list of wire statuses a board column paginates, paged in turn (HS2-F2N4ZN). Most
 * columns own a single status, but when the "Hide Verified column" project setting merges Verified
 * into Completed, the Completed column pages `completed` first and then `verified`, so verified rows
 * beyond the initial global page remain reachable through that column's own "Load more". Returns an
 * empty list for columns that are not status-scoped, and no `verified` column exists while merged. */
export function boardColumnStatuses(columnId: string, hideVerified: boolean): string[] {
  switch (columnId) {
    case 'not-started':
      return ['not_started'];
    case 'started':
      return ['started'];
    case 'completed':
      return hideVerified ? ['completed', 'verified'] : ['completed'];
    case 'verified':
      return hideVerified ? [] : ['verified'];
    default:
      return [];
  }
}

/** Per-status paging state within a board column: the next-page cursor for that status stream
 * (undefined before it has been paged or once it is exhausted) and whether the server has confirmed
 * the stream has no more rows. */
export interface BoardColumnStream {
  cursor?: string;
  exhausted?: boolean;
}

/** Board views whose columns are status-scoped and therefore paginate per column (the multi-status
 * board). Single-column collection views (backlog/archive/trash) and search results have one column,
 * so they keep the global cursor without the starvation problem. */
export function isPerColumnBoardView(view: TicketView, searchActive: boolean): boolean {
  return !searchActive && view !== 'backlog' && view !== 'archive' && view !== 'trash' && view !== 'errors';
}

/** Per-column pagination state: how many rows have been loaded for the column across all of its
 * statuses (so a background refresh can restore the same length instead of resetting to the baseline),
 * whether every one of its status streams is exhausted, and the per-status paging cursors keyed by
 * wire status. A column with a single status keeps exactly one stream; the merged Completed column
 * keeps `completed` and `verified` streams and pages them in order (HS2-F2N4ZN). */
export interface BoardColumnPage {
  loaded: number;
  exhausted?: boolean;
  streams?: Record<string, BoardColumnStream>;
}

/** The next status stream + cursor a column should fetch, walking its ordered statuses and skipping
 * exhausted ones (the first non-exhausted status, continuing its cursor or starting it fresh), or
 * undefined when every status is exhausted. Pure so it can be transition-matrix tested. */
export function nextBoardColumnFetch(
  statuses: readonly string[],
  page: BoardColumnPage | undefined,
): { status: string; cursor?: string } | undefined {
  for (const status of statuses) {
    const stream = page?.streams?.[status];
    if (stream?.exhausted) continue;
    return { status, cursor: stream?.cursor };
  }
  return undefined;
}

/** Fold a completed fetch of one status stream into a column's page state: record that stream's new
 * cursor and exhaustion, recompute whole-column exhaustion across the ordered statuses, and carry the
 * caller-computed total loaded count. Pure so the multi-status walk can be exhaustively tested. */
export function applyBoardColumnFetch(
  statuses: readonly string[],
  page: BoardColumnPage | undefined,
  status: string,
  nextCursor: string | undefined,
  loaded: number,
): BoardColumnPage {
  const streams: Record<string, BoardColumnStream> = { ...(page?.streams ?? {}) };
  streams[status] = { cursor: nextCursor, exhausted: !nextCursor };
  const exhausted = statuses.every((item) => (streams[item] as BoardColumnStream | undefined)?.exhausted === true);
  return { loaded, exhausted, streams };
}

/** Whether a column still has more rows to load: its authoritative total (from the whole-checkout
 * counts summary) exceeds the rows loaded so far, and the server has not already confirmed the stream
 * is exhausted (which guards against a slightly-stale total leaving a no-op "Load more" behind). */
export function boardColumnHasMore(total: number, loaded: number, page: BoardColumnPage | undefined): boolean {
  return total > loaded && page?.exhausted !== true;
}
