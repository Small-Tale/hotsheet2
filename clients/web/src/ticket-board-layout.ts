import type { TicketRow } from './api';
import type { TicketView } from './ticket-views';

export interface TicketBoardGroup {
  id: string;
  title: string;
  tickets: TicketRow[];
}

export function ticketBoardGroups(
  tickets: readonly TicketRow[],
  view: TicketView,
  hideVerified: boolean,
): TicketBoardGroup[] {
  if (view === 'backlog') return [{ id: 'backlog', title: 'Backlog', tickets: [...tickets] }];
  if (view === 'archive') return [{ id: 'archive', title: 'Archive', tickets: [...tickets] }];

  const completedStatuses = hideVerified ? ['completed', 'verified'] : ['completed'];
  const groups: TicketBoardGroup[] = [
    { id: 'not-started', title: 'Not Started', tickets: tickets.filter(ticket => ticket.status === 'not_started') },
    { id: 'started', title: 'Started', tickets: tickets.filter(ticket => ticket.status === 'started') },
    { id: 'completed', title: 'Completed', tickets: tickets.filter(ticket => completedStatuses.includes(ticket.status ?? '')) },
  ];

  if (!hideVerified) {
    groups.push({ id: 'verified', title: 'Verified', tickets: tickets.filter(ticket => ticket.status === 'verified') });
  }
  return groups;
}
