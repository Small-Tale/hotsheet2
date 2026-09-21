import type { TicketRow as WireTicketRow } from './api';

interface PendingCreation {
  projectId: string;
  row: WireTicketRow;
  createdAt: number;
}

/** How long a locally created ticket is retained before we assume the index has caught up. */
const PENDING_TTL_MS = 30_000;

/**
 * Retains tickets just created locally so an eventually-consistent server index that has not
 * yet indexed them cannot drop the optimistic row on a background refresh (HS2-Y5PDHW).
 *
 * The local-creation barrier only holds background refreshes for the create round-trip plus a
 * render frame, but the git-backed store can re-index a moment later; a refresh triggered by
 * overflow, unrelated lease churn, or a poll reconnect then fetches a page that omits the new
 * ticket and blindly replaces the row list, so the ticket vanishes until the next refresh. This
 * keeps each created row in every replaced list until a fetched page actually contains it (the
 * authoritative server row then takes over) or the TTL expires.
 */
export class PendingCreatedTickets {
  private pending: PendingCreation[] = [];

  /** Record a locally created row so later refreshes keep it until the index reflects it. */
  register(projectId: string, row: WireTicketRow, now = Date.now()): void {
    this.prune(now);
    this.pending = this.pending.filter((item) => !(item.projectId === projectId && item.row.id === row.id));
    this.pending.push({ projectId, row, createdAt: now });
  }

  /**
   * The locally created rows for a project that a freshly fetched index does not yet include,
   * newest first, so callers can prepend them to the fetched list. Rows the fetch now contains
   * (index caught up) or past the TTL are dropped from the pending set as a side effect.
   */
  retain(projectId: string, fetched: readonly WireTicketRow[], now = Date.now()): WireTicketRow[] {
    this.prune(now);
    const fetchedIds = new Set(fetched.map((ticket) => ticket.id));
    const kept: WireTicketRow[] = [];
    this.pending = this.pending.filter((item) => {
      if (item.projectId !== projectId) return true;
      if (fetchedIds.has(item.row.id)) return false; // the index caught up; the server row wins.
      kept.push(item.row);
      return true;
    });
    return kept.reverse();
  }

  /** Drop every pending row for a project (e.g. when the project tab closes). */
  forgetProject(projectId: string): void {
    this.pending = this.pending.filter((item) => item.projectId !== projectId);
  }

  private prune(now: number): void {
    this.pending = this.pending.filter((item) => now - item.createdAt <= PENDING_TTL_MS);
  }
}

/**
 * Prepend retained locally created rows (already filtered to those the fetch omits) to a freshly
 * fetched page, skipping any that the page does include so a row is never duplicated.
 */
export function mergeRetainedCreatedRows(
  fetched: readonly WireTicketRow[],
  retained: readonly WireTicketRow[],
): WireTicketRow[] {
  if (retained.length === 0) return [...fetched];
  const fetchedIds = new Set(fetched.map((ticket) => ticket.id));
  return [...retained.filter((row) => !fetchedIds.has(row.id)), ...fetched];
}
