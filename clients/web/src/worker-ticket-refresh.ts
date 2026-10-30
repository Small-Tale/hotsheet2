import type { FullTicket } from './api';
import type { TicketDetailCache } from './ticket-detail-cache';

/** HS2-J118P3: cursor reads share one owner and cannot publish across local saves or disposal. */
export function createWorkerTicketRefresh(options: {
  load: (qualifiedId: string) => Promise<FullTicket>;
  waitUntilSafe: () => Promise<void>;
  publish: (ticket: FullTicket) => void;
}) {
  const desired = new Map<string, string>();
  const pending = new Set<string>();
  let mutationGeneration = 0;
  let disposed = false;
  const stopped = () => disposed;

  async function refresh(qualifiedId: string) {
    if (pending.has(qualifiedId) || stopped()) return;
    pending.add(qualifiedId);
    try {
      while (!stopped()) {
        const waitingGeneration = mutationGeneration;
        await options.waitUntilSafe();
        if (stopped()) return;
        if (mutationGeneration !== waitingGeneration) continue;
        const cursor = desired.get(qualifiedId),
          generation = mutationGeneration;
        let ticket: FullTicket | undefined;
        try {
          ticket = await options.load(qualifiedId);
        } catch {
          // Keep the last detail; a later cursor retries a failed read without idle requests.
        }
        if (stopped()) return;
        if (desired.get(qualifiedId) !== cursor || mutationGeneration !== generation) continue;
        if (ticket?.qualified_id === qualifiedId) options.publish(ticket);
        return;
      }
    } finally {
      pending.delete(qualifiedId);
    }
  }

  return {
    request(qualifiedId: string, cursor: string) {
      if (disposed || desired.get(qualifiedId) === cursor) return;
      desired.set(qualifiedId, cursor);
      void refresh(qualifiedId);
    },
    /** In-flight reads retry after the write barrier settles, even without another worker cursor. */
    invalidate() {
      mutationGeneration += 1;
    },
    dispose() {
      disposed = true;
      desired.clear();
    },
  };
}

/** Remember authoritative detail, reconciling only the exact current inspector's existing drafts. */
export function reconcileWorkerTicketSelection(options: {
  projectId: string;
  ticket: FullTicket;
  selectedProjectId: string | undefined;
  selectedTicket: FullTicket | null;
  cache: TicketDetailCache;
  reconcile: (previous: FullTicket, refreshed: FullTicket) => void;
}) {
  const { projectId, ticket, selectedProjectId, selectedTicket, cache, reconcile } = options;
  cache.put(projectId, ticket);
  if (selectedProjectId === projectId && selectedTicket?.qualified_id === ticket.qualified_id)
    reconcile(selectedTicket, ticket);
}
