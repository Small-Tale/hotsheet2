import type { Capabilities, TicketRow } from './api';
import type { TicketPatch } from './ticket-operations';

export type BulkTicketAction =
  | { kind: 'field'; field: 'category' | 'priority' | 'status'; value: string }
  | { kind: 'up-next'; value: boolean }
  | { kind: 'add-tag'; tag: string }
  | { kind: 'remove-tag'; tag: string }
  | { kind: 'delete' };

/**
 * Runs bulk mutations in request order within a project. A later action starts only
 * after the prior action has committed (or rolled back), so it reads the resulting
 * concurrency tokens instead of the prior action's optimistic snapshot.
 */
export class BulkTicketMutationSequencer {
  private readonly tails = new Map<string, Promise<void>>();

  enqueue<T>(projectId: string, task: () => Promise<T>): Promise<T> {
    const prior = this.tails.get(projectId) ?? Promise.resolve();
    const result = prior.then(task, task);
    const tail = result.then(() => undefined, () => undefined);
    this.tails.set(projectId, tail);
    void tail.then(() => {
      if (this.tails.get(projectId) === tail) this.tails.delete(projectId);
    });
    return result;
  }
}

/** Bulk editing is offered when every selected ticket's provider can update tickets. */
export function canBulkUpdate(
  tickets: readonly TicketRow[],
  capabilitiesFor: (connectionId: string) => Capabilities | undefined,
): boolean {
  return tickets.length > 0 && tickets.every(ticket => {
    const capabilities = capabilitiesFor(ticket.connection_id);
    return Boolean(capabilities?.update);
  });
}

/** One atomic request is safe only when every selected provider advertises it. */
export function canAtomicallyBulkUpdate(
  tickets: readonly TicketRow[],
  capabilitiesFor: (connectionId: string) => Capabilities | undefined,
): boolean {
  return canBulkUpdate(tickets, capabilitiesFor) && tickets.every(ticket => capabilitiesFor(ticket.connection_id)?.atomic_batch === true);
}

export function bulkTagChoices(tickets: readonly TicketRow[]): string[] {
  return [...new Set(tickets.flatMap(ticket => ticket.tags))].sort((left, right) => left.localeCompare(right));
}

export function bulkTicketPatch(ticket: Pick<TicketRow, 'tags'>, action: BulkTicketAction): TicketPatch | undefined {
  if (action.kind === 'field') return { [action.field]: action.value };
  if (action.kind === 'up-next') return { up_next: action.value };
  if (action.kind === 'delete') return { status: 'deleted' };
  const tag = action.tag.trim();
  if (!tag) return undefined;
  if (action.kind === 'add-tag') {
    if (ticket.tags.includes(tag)) return undefined;
    return { tags: [...ticket.tags, tag] };
  }
  if (!ticket.tags.includes(tag)) return undefined;
  return { tags: ticket.tags.filter(value => value !== tag) };
}
