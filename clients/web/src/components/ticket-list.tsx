import { TicketRow, type TicketRowProps } from './ticket-row';
import './ticket-list.css';

export interface TicketListProps {
  tickets: TicketRowProps[];
  label?: string;
}

export function TicketList({ tickets, label = 'Tickets' }: TicketListProps) {
  return <section class="ticket-list" data-component="ticket-list" role="listbox" aria-label={label} aria-multiselectable="true">
    {tickets.map(ticket => <TicketRow {...ticket} />)}
  </section>;
}
