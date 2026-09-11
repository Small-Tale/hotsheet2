import type { Api, CheckoutTicketCounts, CheckoutTicketQuery, CorruptTicket, TicketRow } from './api';

export interface ProjectTicketRefresh {
  corruptTickets?: CorruptTicket[];
  corruptTicketsError?: string;
  tickets?: TicketRow[];
  ticketCounts?: CheckoutTicketCounts;
  nextCursor?: string;
  ticketsError?: string;
}

const message = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);

/** Load healthy and corrupt ticket indexes without either request suppressing the other. */
export async function loadProjectTicketRefresh(client: Pick<Api, 'checkoutCorruptTickets'|'checkoutTicketPage'>, checkout: string, query:CheckoutTicketQuery={collection:'queue'}): Promise<ProjectTicketRefresh> {
  const [tickets, corruptTickets] = await Promise.allSettled([
    client.checkoutTicketPage(checkout,200,undefined,query),
    client.checkoutCorruptTickets(checkout),
  ]);
  const diagnostics=corruptTickets.status === 'fulfilled' ? corruptTickets.value : undefined;
  const corruptSlugs=new Set(diagnostics?.flatMap(item=>item.slug?[item.slug]:[])??[]);
  return {
    ...(tickets.status === 'fulfilled' ? { tickets: tickets.value.items.filter(ticket=>!corruptSlugs.has(ticket.slug)), ticketCounts:tickets.value.counts, nextCursor:tickets.value.next_cursor } : { ticketsError: message(tickets.reason) }),
    ...(corruptTickets.status === 'fulfilled' ? { corruptTickets: corruptTickets.value } : { corruptTicketsError: message(corruptTickets.reason) }),
  };
}
