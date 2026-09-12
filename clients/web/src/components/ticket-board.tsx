import './ticket-board.css';

import { TicketBoardColumn, type TicketBoardColumnProps } from './ticket-board-column';
import { TicketEmptyState,type TicketEmptyStateProps } from './ticket-empty-state';

export type TicketColumnProps = TicketBoardColumnProps;

export interface TicketBoardProps {
  columns: TicketColumnProps[];
  label?: string;
  emptyState?: TicketEmptyStateProps;
  continuation?:{loading:boolean};
}

export function TicketBoard({ columns, label = 'Ticket board',emptyState,continuation }: TicketBoardProps) {
  const empty=columns.every(column=>(column.totalCount??column.tickets.length)===0);
  return <section class="ticket-board" data-key="ticket-board" data-component="ticket-board" data-ticket-selection-root="true" role="listbox" aria-multiselectable="true" aria-label={label}>
    <div class="ticket-board__columns" style={`--ticket-board-column-count:${columns.length};--ticket-board-min-width:${columns.length * 250}px`}>
      {columns.map(column => <TicketBoardColumn {...column} selectionRoot={false}/>)}
    </div>
    {continuation&&<button type="button" class="ticket-page-more ticket-board__more" data-action="load-next-ticket-page" disabled={continuation.loading}>{continuation.loading?'Loading…':'Load more tickets'}</button>}
    {empty&&emptyState&&<div class="ticket-board__empty"><TicketEmptyState {...emptyState}/></div>}
  </section>;
}
