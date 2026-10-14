import type { FullTicket, TicketRow } from './api';

/**
 * Bounded recently-viewed ticket detail cache (HS2-4Q04Z9). Opening a ticket the user viewed recently
 * renders its last known full detail immediately instead of the inspector loading skeleton; the caller
 * still refreshes it from the server in the background. Entries are keyed by project and qualified id
 * and evicted least-recently-used first.
 *
 * Freshness comes from the live ticket list: list rows are kept current by the change stream, so a row
 * whose `updated_at` differs from the cached detail marks the entry stale. A stale entry is still shown
 * (the row's live summary fields overlay it, so title/status/tags are never older than the list) and the
 * background refresh replaces it.
 */
export const TICKET_DETAIL_CACHE_LIMIT = 50;

export interface CachedTicketDetail {
  ticket: FullTicket;
  /** Whether the cached detail matches the live list row's revision. */
  fresh: boolean;
}

/** Live summary fields a list row carries that may overlay a stale cached detail. */
const ROW_OVERLAY_FIELDS = [
  'title',
  'category',
  'priority',
  'status',
  'started_phase',
  'up_next',
  'feedback_needed',
  'tags',
  'blocked_by',
  'blocked_reason',
  'claimed_by',
  'claim_lease_expires_at',
  'claim_eta_at',
  'claim_started_at',
  'worker_label',
  'claim_count',
  'latest_confidence',
  'updated_at',
  'completed_at',
  'closed_at',
  'close_reason',
  'duplicate_of',
] as const satisfies readonly (keyof TicketRow & keyof FullTicket)[];

export function ticketDetailCacheKey(projectId: string, qualifiedId: string) {
  return `${projectId}\u0000${qualifiedId}`;
}

export function createTicketDetailCache(limit = TICKET_DETAIL_CACHE_LIMIT) {
  const entries = new Map<string, FullTicket>();
  function touch(key: string, ticket: FullTicket) {
    entries.delete(key);
    entries.set(key, ticket);
    while (entries.size > limit) entries.delete(entries.keys().next().value!);
  }
  return {
    /** Remember (or replace) a ticket's full detail, marking it most recently used. */
    put(projectId: string, ticket: FullTicket) {
      touch(ticketDetailCacheKey(projectId, ticket.qualified_id), ticket);
    },
    /** The cached detail for a listed row, overlaid with the row's live fields when it is stale. */
    get(projectId: string, row: TicketRow): CachedTicketDetail | undefined {
      const key = ticketDetailCacheKey(projectId, row.qualified_id),
        cached = entries.get(key);
      if (!cached) return;
      touch(key, cached);
      if (cached.updated_at === row.updated_at) return { ticket: cached, fresh: true };
      const overlay: Partial<FullTicket> = {};
      for (const field of ROW_OVERLAY_FIELDS) (overlay as Record<string, unknown>)[field] = row[field];
      if (row.blocked_reason === undefined) delete overlay.blocked_reason;
      return { ticket: { ...cached, ...overlay }, fresh: false };
    },
    delete(projectId: string, qualifiedId: string) {
      entries.delete(ticketDetailCacheKey(projectId, qualifiedId));
    },
    clear() {
      entries.clear();
    },
    get size() {
      return entries.size;
    },
  };
}

export type TicketDetailCache = ReturnType<typeof createTicketDetailCache>;

/** Whether a background refresh returned the same revision the inspector already shows. */
export function sameTicketRevision(shown: FullTicket, refreshed: FullTicket) {
  return (
    shown.qualified_id === refreshed.qualified_id &&
    shown.updated_at === refreshed.updated_at &&
    shown.concurrency_token === refreshed.concurrency_token &&
    shown.notes.length === refreshed.notes.length &&
    shown.attachments.length === refreshed.attachments.length
  );
}
