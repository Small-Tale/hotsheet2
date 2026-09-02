import './ticket-list.css';

import type { CorruptTicket } from '../api';
import { CorruptTicketRow } from './corrupt-ticket-row';
import { TicketRow, type TicketRowProps } from './ticket-row';

export interface TicketListProps {
  tickets: TicketRowProps[];
  corruptTickets?: CorruptTicket[];
  label?: string;
}

export function TicketList({ tickets, corruptTickets = [], label = 'Tickets' }: TicketListProps) {
  return <section class="ticket-list" data-component="ticket-list" data-ticket-selection-root="true" role="listbox" aria-label={label} aria-multiselectable="true">
    {corruptTickets.map(ticket => <CorruptTicketRow ticket={ticket} />)}
    {tickets.map(ticket => <TicketRow {...ticket} presentation="list" />)}
  </section>;
}
