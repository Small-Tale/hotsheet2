import type { TicketRow } from './api';

export type TicketView = 'all' | 'backlog' | 'archive';

export const canCreateTicketInView = (view: TicketView): boolean => view !== 'archive';
export const newTicketStatusForView = (view: TicketView): 'not_started' | 'backlog' => view === 'backlog' ? 'backlog' : 'not_started';

export function isOpenTicket(ticket: TicketRow): boolean {
  return !['completed', 'verified', 'archive', 'deleted', 'moved'].includes(ticket.status ?? 'not_started');
}

export function isUpNextTicket(ticket: TicketRow): boolean {
  return ticket.up_next && isOpenTicket(ticket);
}

export function isArchivedTicket(ticket: TicketRow): boolean {
  return ['archive', 'deleted', 'moved'].includes(ticket.status ?? '');
}

export function isQueuedTicket(ticket: TicketRow): boolean {
  return ticket.status !== 'backlog' && !isArchivedTicket(ticket);
}

export function ticketsForView(tickets: readonly TicketRow[], view: TicketView): TicketRow[] {
  if (view === 'archive') return tickets.filter(isArchivedTicket);
  if (view === 'backlog') return tickets.filter(ticket => ticket.status === 'backlog');
  return tickets.filter(isQueuedTicket);
}
