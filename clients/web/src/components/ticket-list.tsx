import './ticket-list.css';

import type { CorruptTicket } from '../api';
import { corruptTicketKey, type CorruptTicketRecoveryState,CorruptTicketRow } from './corrupt-ticket-row';
import { TicketRow, type TicketRowProps } from './ticket-row';

export interface TicketListProps {
  tickets: TicketRowProps[];
  corruptTickets?: CorruptTicket[];
  corruptRecovery?: Record<string,CorruptTicketRecoveryState>;
  label?: string;
}

export function TicketList({ tickets, corruptTickets = [], corruptRecovery = {}, label = 'Tickets' }: TicketListProps) {
  return <section class="ticket-list" data-component="ticket-list">
    {corruptTickets.length>0&&<div class="ticket-list__diagnostics" aria-label="Unreadable tickets">{corruptTickets.map(ticket => <CorruptTicketRow ticket={ticket} recovery={corruptRecovery[corruptTicketKey(ticket)]} />)}</div>}
    <div class="ticket-list__tickets" data-ticket-selection-root="true" role="listbox" aria-label={label} aria-multiselectable="true">{tickets.map(ticket => <TicketRow {...ticket} presentation="list" />)}</div>
  </section>;
}
