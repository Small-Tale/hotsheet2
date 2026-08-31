import './ticket-list.css';

import { TicketRow, type TicketRowProps } from './ticket-row';

export interface TicketListProps {
  tickets: TicketRowProps[];
  label?: string;
}

export function TicketList({ tickets, label = 'Tickets' }: TicketListProps) {
  return <section class="ticket-list" data-component="ticket-list" data-ticket-selection-root="true" role="listbox" aria-label={label} aria-multiselectable="true">
    {tickets.map(ticket => <TicketRow {...ticket} presentation="list" />)}
  </section>;
}
