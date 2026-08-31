import type { FullTicket, TicketRow } from './api';
import type { TicketPatch } from './ticket-operations';

const projectedFields = new Set(['title', 'details', 'category', 'priority', 'status', 'tags', 'up_next', 'blocked_by', 'blocked_reason']);

export function projectTicketPatch<T extends TicketRow>(ticket: T, patch: TicketPatch): T {
  const projected = Object.fromEntries(Object.entries(patch).filter(([key]) => projectedFields.has(key)));
  return { ...ticket, ...projected };
}

export function ticketRowFromFull(previous: TicketRow, ticket: FullTicket): TicketRow {
  return { ...previous, ...ticket };
}

export interface MutationTiming {
  slug: string;
  optimistic_ms: number;
  request_ms: number;
  outcome: 'committed' | 'rolled_back' | 'stale';
}

export function reportMutationTiming(timing: MutationTiming): void {
  document.dispatchEvent(new CustomEvent<MutationTiming>('hotsheet:mutation-timing', { detail: timing }));
}
