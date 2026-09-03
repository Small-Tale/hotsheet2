import type { TicketRow } from './api';

const dependencyDone = (ticket: TicketRow) => ticket.status === 'completed' || ticket.status === 'verified';

/** Match the core engine: a dependency blocks until its ticket is Completed or Verified. */
export function hasUnresolvedBlocker(ticket: TicketRow, tickets: readonly TicketRow[]): boolean {
  if (ticket.blocked_by.length === 0) return false;
  const done = new Set(tickets.filter(dependencyDone).flatMap(item => [item.id, item.native_id, item.slug]));
  return ticket.blocked_by.some(id => !done.has(id));
}
