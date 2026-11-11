import { signal } from 'kerfjs';

import type { TicketStoreSyncReport } from '../api';
import type { Project } from '../interactions/types';

/** The request a load issues; injected so tests run without a server. */
export type LoadTicketStoreSync = (target: Project) => Promise<TicketStoreSyncReport>;

/**
 * Per-project ticket-store sync health (HS2-ADHFKZ). Loaded once when a project is first shown
 * and again only when the change stream reports a `ticket_store_sync` event, so it never polls.
 * A failed load keeps the last report: a transient request error must not hide or invent a
 * publication failure.
 */
export function createTicketStoreSync(request: LoadTicketStoreSync) {
  const byProject = signal<Record<string, TicketStoreSyncReport>>({});
  const requested = new Set<string>();
  // Monotonic across resets, so a response requested before a reset never matches a later load.
  let nextGeneration = 0;
  const generations = new Map<string, number>();

  async function load(target: Project): Promise<void> {
    nextGeneration += 1;
    const generation = nextGeneration;
    generations.set(target.id, generation);
    let report: TicketStoreSyncReport;
    try {
      report = await request(target);
    } catch {
      return;
    }
    // An older response that lands after a newer one must not overwrite it.
    if (generations.get(target.id) !== generation) return;
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
    byProject.value = {};
    requested.clear();
    generations.clear();
  }

  return { load, loadOnce, report, reset };
}
