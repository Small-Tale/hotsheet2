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

/** The wire status a board column paginates independently, or undefined for a column that is not
 * status-scoped (single-collection board views paginate globally instead). */
export function boardColumnStatus(columnId: string): string | undefined {
  switch (columnId) {
    case 'not-started': return 'not_started';
    case 'started': return 'started';
    case 'completed': return 'completed';
    case 'verified': return 'verified';
    default: return undefined;
  }
}

/** Board views whose columns are status-scoped and therefore paginate per column (the multi-status
 * board). Single-column collection views (backlog/archive/trash) and search results have one column,
 * so they keep the global cursor without the starvation problem. */
export function isPerColumnBoardView(view: TicketView, searchActive: boolean): boolean {
  return !searchActive && view !== 'backlog' && view !== 'archive' && view !== 'trash' && view !== 'errors';
}

/** Per-column pagination state: the next-page cursor for the column's status stream (undefined before
 * the column has been independently paged, or once the stream is exhausted), how many rows have been
 * loaded for it (so a background refresh can restore the same length instead of resetting to the
 * baseline), and whether the server has confirmed there are no more rows. */
export interface BoardColumnPage { cursor?: string; loaded: number; exhausted?: boolean }

/** Whether a column still has more rows to load: its authoritative total (from the whole-checkout
 * counts summary) exceeds the rows loaded so far, and the server has not already confirmed the stream
 * is exhausted (which guards against a slightly-stale total leaving a no-op "Load more" behind). */
export function boardColumnHasMore(total: number, loaded: number, page: BoardColumnPage | undefined): boolean {
  return total > loaded && page?.exhausted !== true;
}
