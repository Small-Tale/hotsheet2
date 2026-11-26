import { signal } from 'kerfjs';

import type { TicketStoreSyncReport } from '../api';
import { type BackoffDelay, retryTransient } from '../bounded-backoff';
import type { Project } from '../interactions/types';

/** The request a load issues; injected so tests run without a server. */
export type LoadTicketStoreSync = (target: Project) => Promise<TicketStoreSyncReport>;

export interface TicketStoreSyncOptions {
  /** HS2-JGFPG4: a load that failed past its retries; the caller shows it through its status path. */
  onPersistentFailure?: (target: Project, reason: unknown) => void;
  /** The bounded backoff's wait between retries; injected so tests need no timers. */
  delay?: BackoffDelay;
}

/**
 * Per-project ticket-store sync health (HS2-ADHFKZ). Loaded once when a project is first shown
 * and again only when the change stream reports a `ticket_store_sync` event, so it never polls.
 * A failed load keeps the last report: a transient request error must not hide or invent a
 * publication failure. HS2-JGFPG4: a transient failure is retried on the shared bounded backoff;
 * one that persists is reported once and forgets the first-show request, so showing the project
 * again asks again.
 */
export function createTicketStoreSync(request: LoadTicketStoreSync, options: TicketStoreSyncOptions = {}) {
  const byProject = signal<Record<string, TicketStoreSyncReport>>({});
  const requested = new Set<string>();
  // Monotonic across resets, so a response requested before a reset never matches a later load.
  let nextGeneration = 0;
  const generations = new Map<string, number>();
  // Aborted by a reset, so a retry waiting on the backoff ends at once.
  let retries = new AbortController();

  async function load(target: Project): Promise<void> {
    nextGeneration += 1;
    const generation = nextGeneration;
    generations.set(target.id, generation);
    const current = () => generations.get(target.id) === generation;
    const signal = retries.signal;
    let report: TicketStoreSyncReport;
    try {
      report = await retryTransient(() => request(target), { signal, delay: options.delay, wanted: current });
    } catch (reason) {
      // A reset or a newer load owns the project now; only this load's own failure is reported.
      if (signal.aborted || !current()) return;
      requested.delete(target.id);
      options.onPersistentFailure?.(target, reason);
      return;
    }
    // An older response that lands after a newer one must not overwrite it.
    if (!current()) return;
    byProject.value = { ...byProject.peek(), [target.id]: report };
  }

  /** Load the project's health the first time it is shown; later loads come from change events. */
  function loadOnce(target: Project): void {
    if (requested.has(target.id)) return;
    requested.add(target.id);
    void load(target);
  }

  const report = (projectId: string): TicketStoreSyncReport | undefined => byProject.value[projectId];

  function reset(): void {
    retries.abort();
    retries = new AbortController();
    byProject.value = {};
    requested.clear();
    generations.clear();
  }

  return { load, loadOnce, report, reset };
}
