import './ticket-board.css';

import { TicketBoardColumn, type TicketBoardColumnProps } from './ticket-board-column';
import { TicketEmptyState,type TicketEmptyStateProps } from './ticket-empty-state';

export type TicketColumnProps = TicketBoardColumnProps;

export interface TicketBoardProps {
  columns: TicketColumnProps[];
  label?: string;
  emptyState?: TicketEmptyStateProps;
}

export function TicketBoard({ columns, label = 'Ticket board',emptyState={kind:'view'} }: TicketBoardProps) {
  const empty=columns.every(column=>column.tickets.length===0);
  return <section class="ticket-board" data-key="ticket-board" data-component="ticket-board" data-ticket-selection-root="true" role="listbox" aria-multiselectable="true" aria-label={label}>
    <div class="ticket-board__columns" style={`--ticket-board-column-count:${columns.length};--ticket-board-min-width:${columns.length * 250}px`}>
      {columns.map(column => <TicketBoardColumn {...column} selectionRoot={false} emptyPlaceholder={!empty}/>)}
    </div>
    {empty&&<div class="ticket-board__empty"><TicketEmptyState {...emptyState}/></div>}
  </section>;
}
