import './ticket-list.css';

import type { CorruptTicket } from '../api';
import { corruptTicketKey, type CorruptTicketRecoveryState,CorruptTicketRow } from './corrupt-ticket-row';
import { TicketEmptyState,type TicketEmptyStateProps } from './ticket-empty-state';
import { TicketRow, type TicketRowProps } from './ticket-row';

export interface TicketListProps {
  tickets: TicketRowProps[];
  totalCount?: number;
  corruptTickets?: CorruptTicket[];
  corruptRecovery?: Record<string,CorruptTicketRecoveryState>;
  selectedCorruptKey?: string;
  label?: string;
  emptyState?: TicketEmptyStateProps;
}

export function TicketList({ tickets, totalCount = tickets.length, corruptTickets = [], corruptRecovery = {}, selectedCorruptKey, label = 'Tickets',emptyState }: TicketListProps) {
  const empty=tickets.length===0&&corruptTickets.length===0;
  const loadingMore=totalCount>tickets.length;
  return <section class="ticket-list" data-key="ticket-list" data-component="ticket-list" data-empty={empty?'true':undefined} data-rendered-count={tickets.length} data-total-count={totalCount}>
    {corruptTickets.length>0&&<div class="ticket-list__diagnostics" aria-label="Unreadable tickets">{corruptTickets.map(ticket => <CorruptTicketRow ticket={ticket} recovery={corruptRecovery[corruptTicketKey(ticket)]} selected={selectedCorruptKey===corruptTicketKey(ticket)} />)}</div>}
    <div class="ticket-list__tickets" data-ticket-selection-root="true" role="listbox" aria-label={label} aria-multiselectable="true">{empty&&emptyState?<TicketEmptyState {...emptyState}/>:tickets.map(ticket => <TicketRow {...ticket} presentation="list" />)}{loadingMore&&<div class="ticket-list__progress" data-ticket-progressive-loading="true" role="status">Loading more tickets…</div>}</div>
  </section>;
}
