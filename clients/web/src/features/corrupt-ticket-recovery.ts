import type { Signal } from 'kerfjs';
import { signal } from 'kerfjs';

import { Api, type CorruptTicket, revealCorruptTicketFile } from '../api';
import { corruptTicketKey, type CorruptTicketRecoveryState } from '../components/corrupt-ticket-row';
import type { Project } from '../interactions/types';

/** Live application bindings the corrupt-ticket recovery owner reads (HS2-K7SYHQ). */
export interface CorruptTicketRecoveryDependencies {
  project: () => Project | undefined;
  corruptTickets: Signal<CorruptTicket[]>;
  showToast: (message: string) => void;
  /** Reload the active project quietly after a repair ticket was queued. */
  refreshProject: () => Promise<unknown>;
}

/**
 * Owns per-file corrupt-ticket recovery state and its reveal / queue-AI-repair actions,
 * extracted from the application runtime (HS2-K7SYHQ).
 */
export function createCorruptTicketRecoveryController(dependencies: CorruptTicketRecoveryDependencies) {
  const { project, corruptTickets, showToast, refreshProject } = dependencies;
  const corruptRecovery = signal<Record<string, CorruptTicketRecoveryState>>({});
  function setCorruptRecovery(key: string, value: CorruptTicketRecoveryState) {
    corruptRecovery.value = { ...corruptRecovery.value, [key]: value };
  }
  async function revealCorruptTicket(key: string) {
    const current = project(),
      ticket = corruptTickets.value.find((item) => corruptTicketKey(item) === key);
    // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
    if (!current || !ticket || corruptRecovery.value[key]?.pending) return;
    setCorruptRecovery(key, { pending: 'reveal' });
    try {
      await revealCorruptTicketFile(current.id, ticket.path);
      setCorruptRecovery(key, {});
      showToast('Opened the file location.');
    } catch (reason) {
      setCorruptRecovery(key, { message: reason instanceof Error ? reason.message : String(reason), failed: true });
    }
  }
  async function queueCorruptTicketRepair(key: string) {
    const current = project(),
      ticket = corruptTickets.value.find((item) => corruptTicketKey(item) === key);
    // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
    if (!current || !ticket || ticket.error_code === 'upgrade_required' || corruptRecovery.value[key]?.pending) return;
    setCorruptRecovery(key, { pending: 'repair' });
    try {
      const created = await new Api(current.apiPath).createCorruptTicketRepair(current.id, ticket.path);
      setCorruptRecovery(key, {});
      showToast(`Queued ${created.slug} for AI repair.`);
      if (project()?.id === current.id) await refreshProject();
    } catch (reason) {
      setCorruptRecovery(key, { message: reason instanceof Error ? reason.message : String(reason), failed: true });
    }
  }
  return { corruptRecovery, revealCorruptTicket, queueCorruptTicketRepair };
}
