import type { Capabilities, CheckoutTicketCounts, TicketRow } from './api';
import type { TicketGroupMenuModel } from './components/ticket-row-context-menu';
import { ticketGroupKey, type TicketGroupOption, ticketGroupValue } from './ticket-groups';
import type { TicketPatch } from './ticket-operations';
import { isArchivedTicket, isOpenTicket, isQueuedTicket, isUpNextTicket } from './ticket-views';

export type BulkTicketAction =
  | { kind: 'field'; field: 'category' | 'priority' | 'status'; value: string }
  | { kind: 'up-next'; value: boolean }
  | { kind: 'reopen' }
  | { kind: 'add-tag'; tag: string }
  | { kind: 'remove-tag'; tag: string }
  /** Set (a stored value: an id or a name) or clear (null) the group, optionally with a board column's status (HS2-PRXD39). */
  | { kind: 'group'; group: string | null; status?: string }
  | { kind: 'delete' };

/**
 * Runs bulk mutations in request order within a project. A later action starts only
 * after the prior action has committed (or rolled back), so it reads the resulting
 * concurrency tokens instead of the prior action's optimistic snapshot.
 */
export class BulkTicketMutationSequencer {
  private readonly tails = new Map<string, Promise<void>>();

  enqueue<T>(projectId: string, task: () => Promise<T>): Promise<T> {
    const prior = this.tails.get(projectId);
    // An idle queue can publish its optimistic state in the initiating event. Deferring it
    // to a promise turn creates a separate render for context-menu dismissal first.
    let result: Promise<T>;
    if (prior) result = prior.then(task, task);
    else {
      try {
        result = task();
      } catch (error) {
        result = Promise.reject(error instanceof Error ? error : new Error(String(error)));
      }
    }
    const tail = result.then(
      () => undefined,
      () => undefined,
    );
    this.tails.set(projectId, tail);
    void tail.then(() => {
      if (this.tails.get(projectId) === tail) this.tails.delete(projectId);
    });
    return result;
  }
}

/** Keep board totals useful while a bulk response and its replacement page are pending. */
export function projectBulkTicketCounts(
  counts: CheckoutTicketCounts,
  changes: readonly { before: TicketRow; after: TicketRow }[],
): CheckoutTicketCounts {
  const next = { ...counts };
  for (const { before, after } of changes) {
    const delta = (key: keyof CheckoutTicketCounts, was: boolean, now: boolean) => {
      if (was === now || typeof next[key] !== 'number') return;
      (next as unknown as Record<string, number>)[key] = Math.max(0, next[key] + (now ? 1 : -1));
    };
    delta('queued', isQueuedTicket(before), isQueuedTicket(after));
    delta('backlog', before.status === 'backlog', after.status === 'backlog');
    delta('archive', isArchivedTicket(before), isArchivedTicket(after));
    delta('trash', before.status === 'deleted', after.status === 'deleted');
    delta('open', isOpenTicket(before) && isQueuedTicket(before), isOpenTicket(after) && isQueuedTicket(after));
    delta('started', before.status === 'started', after.status === 'started');
    delta('verified', before.status === 'verified', after.status === 'verified');
    delta('up_next', isUpNextTicket(before), isUpNextTicket(after));
  }
  return next;
}

/** Bulk editing is offered when every selected ticket's provider can update tickets. */
export function canBulkUpdate(
  tickets: readonly TicketRow[],
  capabilitiesFor: (connectionId: string) => Capabilities | undefined,
): boolean {
  return (
    tickets.length > 0 &&
    tickets.every((ticket) => {
      const capabilities = capabilitiesFor(ticket.connection_id);
      return Boolean(capabilities?.update);
    })
  );
}

/** One atomic request is safe only when every selected provider advertises it. */
export function canAtomicallyBulkUpdate(
  tickets: readonly TicketRow[],
  capabilitiesFor: (connectionId: string) => Capabilities | undefined,
): boolean {
  return (
    canBulkUpdate(tickets, capabilitiesFor) &&
    tickets.every((ticket) => capabilitiesFor(ticket.connection_id)?.atomic_batch === true)
  );
}

/** Why the tickets cannot be grouped, or undefined when every provider supports it (HS2-TX17CM). */
export function ticketGroupDisabledReason(
  tickets: readonly TicketRow[],
  capabilitiesFor: (connectionId: string) => Capabilities | undefined,
): string | undefined {
  if (!canBulkUpdate(tickets, capabilitiesFor)) return 'One or more selected ticket providers do not support updates.';
  return tickets.every((ticket) => capabilitiesFor(ticket.connection_id)?.groups === true)
    ? undefined
    : 'One or more selected ticket providers do not support groups.';
}

/**
 * The ticket menu's group entries for the target tickets, given every known group (HS2-TX17CM).
 * A duplicate display name shows its id so same-named groups stay distinguishable (HS2-9KZSD4).
 */
export function ticketGroupMenuModel(
  targets: readonly TicketRow[],
  groups: readonly TicketGroupOption[],
  capabilitiesFor: (connectionId: string) => Capabilities | undefined,
): TicketGroupMenuModel | undefined {
  if (targets.length === 0) return undefined;
  const values = targets.map(ticketGroupValue),
    first = values[0],
    shared =
      first !== undefined &&
      values.every((value) => value !== undefined && ticketGroupKey(value) === ticketGroupKey(first)),
    current = shared ? groups.find((group) => ticketGroupKey(group.id) === ticketGroupKey(first)) : undefined;
  return {
    groups: groups.map((group) => ({
      id: group.id,
      label: group.duplicate === true ? `${group.name} (${group.id})` : group.name,
    })),
    current: current?.id ?? (shared ? first : undefined),
    removable: values.some((value) => value !== undefined),
    disabledReason: ticketGroupDisabledReason(targets, capabilitiesFor),
  };
}

export function bulkTagChoices(tickets: readonly TicketRow[]): string[] {
  return [...new Set(tickets.flatMap((ticket) => ticket.tags))].sort((left, right) => left.localeCompare(right));
}

export function bulkTicketPatch(
  ticket: Pick<TicketRow, 'tags' | 'group' | 'status'>,
  action: BulkTicketAction,
): TicketPatch | undefined {
  if (action.kind === 'group') {
    const patch: TicketPatch = {},
      current = ticketGroupValue(ticket);
    // Groups compare by identity: an id exactly, and a legacy name in any casing (HS2-9KZSD4).
    if (
      action.group === null
        ? current !== undefined
        : !current || ticketGroupKey(current) !== ticketGroupKey(action.group)
    )
      patch.group = action.group;
    if (action.status !== undefined && ticket.status !== action.status) patch.status = action.status;
    return Object.keys(patch).length ? patch : undefined;
  }
  if (action.kind === 'field') return { [action.field]: action.value };
  if (action.kind === 'up-next') return { up_next: action.value };
  if (action.kind === 'reopen') return { status: 'not_started', up_next: true };
  if (action.kind === 'delete') return { status: 'deleted' };
  const tag = action.tag.trim();
  if (!tag) return undefined;
  if (action.kind === 'add-tag') {
    if (ticket.tags.includes(tag)) return undefined;
    return { tags: [...ticket.tags, tag] };
  }
  if (!ticket.tags.includes(tag)) return undefined;
  return { tags: ticket.tags.filter((value) => value !== tag) };
}
