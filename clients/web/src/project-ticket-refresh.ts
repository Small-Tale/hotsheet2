import type { Api, CorruptTicket, TicketRow } from './api';

export interface ProjectTicketRefresh {
  corruptTickets?: CorruptTicket[];
  corruptTicketsError?: string;
  tickets?: TicketRow[];
  ticketsError?: string;
}

const message = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);

/** Load healthy and corrupt ticket indexes without either request suppressing the other. */
export async function loadProjectTicketRefresh(client: Pick<Api, 'checkoutCorruptTickets'|'checkoutTickets'>, checkout: string): Promise<ProjectTicketRefresh> {
  const [tickets, corruptTickets] = await Promise.allSettled([
    client.checkoutTickets(checkout),
    client.checkoutCorruptTickets(checkout),
  ]);
  return {
    ...(tickets.status === 'fulfilled' ? { tickets: tickets.value } : { ticketsError: message(tickets.reason) }),
    ...(corruptTickets.status === 'fulfilled' ? { corruptTickets: corruptTickets.value } : { corruptTicketsError: message(corruptTickets.reason) }),
  };
}
