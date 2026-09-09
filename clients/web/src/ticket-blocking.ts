import type { TicketRow } from './api';

/** Match the core engine: visible explanatory text is the source of truth. */
export function hasUnresolvedBlocker(ticket: TicketRow): boolean {
  return Boolean(ticket.blocked_reason?.trim());
}
