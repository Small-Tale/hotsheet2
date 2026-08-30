import './ticket-board-column.css';

import { TicketRow, type TicketRowProps } from './ticket-row';

export interface TicketBoardColumnProps {
  id: string;
  title: string;
  tickets: TicketRowProps[];
}

export function TicketBoardColumn({ id, title, tickets }: TicketBoardColumnProps) {
  return <section class="ticket-board-column" data-component="ticket-board-column" data-column-id={id} aria-labelledby={`ticket-column-${id}`}>
    <header class="ticket-board-column__header">
      <h2 id={`ticket-column-${id}`}>{title}</h2>
      <span aria-label={`${tickets.length} tickets`}>{tickets.length}</span>
    </header>
    <div class="ticket-board-column__tickets" role="listbox" aria-label={`${title} tickets`} aria-multiselectable="true">
      {tickets.map(ticket => <TicketRow {...ticket} />)}
    </div>
  </section>;
}
