import { TicketRow, type TicketRowProps } from './ticket-row';
import './ticket-board.css';

export interface TicketColumnProps {
  id: string;
  title: string;
  tickets: TicketRowProps[];
}

export interface TicketBoardProps {
  columns: TicketColumnProps[];
  label?: string;
}

export function TicketBoard({ columns, label = 'Ticket board' }: TicketBoardProps) {
  return <section class="ticket-board" aria-label={label}>
    {columns.map(column => <section class="ticket-board__column" data-column-id={column.id} aria-labelledby={`ticket-column-${column.id}`}>
      <header class="ticket-board__column-header">
        <h2 id={`ticket-column-${column.id}`}>{column.title}</h2>
        <span aria-label={`${column.tickets.length} tickets`}>{column.tickets.length}</span>
      </header>
      <div class="ticket-board__tickets" role="listbox" aria-label={`${column.title} tickets`} aria-multiselectable="true">
        {column.tickets.map(ticket => <TicketRow {...ticket} />)}
      </div>
    </section>)}
  </section>;
}
