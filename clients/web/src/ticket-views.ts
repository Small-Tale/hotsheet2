import type { CheckoutTicketQuery, TicketRow } from './api';

export type BuiltInTicketView = 'all' | 'backlog' | 'archive' | 'errors';
export type TicketView = BuiltInTicketView | `custom:${string}`;

export const customTicketViewId = (id: string): TicketView => `custom:${id}`;
export const customTicketViewKey = (view: TicketView): string | undefined => view.startsWith('custom:') ? view.slice('custom:'.length) : undefined;

export const canCreateTicketInView = (view: TicketView): boolean => !['archive', 'errors'].includes(view);
export const newTicketStatusForView = (view: TicketView): 'not_started' | 'backlog' => view === 'backlog' ? 'backlog' : 'not_started';
export const newTicketCreationPlacement = (view: TicketView, upNext: boolean) => ({status:upNext?'not_started' as const:newTicketStatusForView(view),up_next:upNext});

/** Match a built-in collection on the server before its page size is applied. */
export function ticketViewQuery(view: TicketView): CheckoutTicketQuery {
  if (view === 'backlog') return { status: 'backlog' };
  if (view === 'archive') return { collection: 'archive' };
  return { collection: 'queue' };
}

export function isOpenTicket(ticket: TicketRow): boolean {
  return ['not_started', 'started'].includes(ticket.status ?? 'not_started');
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
  if (view === 'errors') return [];
  if (view === 'archive') return tickets.filter(isArchivedTicket);
  if (view === 'backlog') return tickets.filter(ticket => ticket.status === 'backlog');
  return tickets.filter(isQueuedTicket);
}

export function selectionVisibleInView(tickets: readonly TicketRow[], selectedSlugs: readonly string[], view: TicketView): string[] {
  const visible = new Set(ticketsForView(tickets, view).map(ticket => ticket.slug));
  return selectedSlugs.filter(slug => visible.has(slug));
}

export function selectionAfterTicketViewChange(current: TicketView, next: TicketView, selectedSlugs: readonly string[]): string[] {
  return current === next ? [...selectedSlugs] : [];
}
