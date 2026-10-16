import type { CheckoutTicketCounts, TicketRow } from './api';
import { ALL_TICKETS_SCOPE, isArchivedTicket, type TicketScope } from './ticket-views';

export interface TicketBoardGroup {
  id: string;
  title: string;
  tickets: TicketRow[];
}

export function ticketBoardGroupTotal(
  id: string,
  loaded: number,
  view: TicketScope,
  counts: CheckoutTicketCounts | undefined,
  hideVerified: boolean,
): number {
  if (!counts) return loaded;
  if (view === 'backlog') return counts.backlog;
  if (view === 'archive') return counts.archive;
  if (view === 'trash') return counts.trash ?? loaded;
  if (view !== 'all') return loaded;
  if (id === 'not-started') return Math.max(0, counts.open - counts.started);
  if (id === 'started') return counts.started;
  if (id === 'completed')
    return hideVerified
      ? Math.max(0, counts.queued - counts.open)
      : counts.verified === undefined
        ? loaded
        : Math.max(0, counts.queued - counts.open - counts.verified);
  if (id === 'verified') return counts.verified ?? loaded;
  return loaded;
}

export function ticketBoardGroups(
  tickets: readonly TicketRow[],
  view: TicketScope,
  hideVerified: boolean,
): TicketBoardGroup[] {
  if (view === 'backlog') return [{ id: 'backlog', title: 'Backlog', tickets: [...tickets] }];
  if (view === 'archive') return [{ id: 'archive', title: 'Archive', tickets: tickets.filter(isArchivedTicket) }];
  if (view === 'trash') return [{ id: 'trash', title: 'Trash', tickets: [...tickets] }];

  const groups: TicketBoardGroup[] = [
    { id: 'not-started', title: 'Not Started', tickets: tickets.filter((ticket) => ticket.status === 'not_started') },
    { id: 'started', title: 'Started', tickets: tickets.filter((ticket) => ticket.status === 'started') },
    {
      id: 'completed',
      title: 'Completed',
      tickets: [
        ...tickets.filter((ticket) => ticket.status === 'completed'),
        ...(hideVerified ? tickets.filter((ticket) => ticket.status === 'verified') : []),
      ],
    },
  ];

  if (!hideVerified) {
    groups.push({
      id: 'verified',
      title: 'Verified',
      tickets: tickets.filter((ticket) => ticket.status === 'verified'),
    });
  }
  // All Tickets brackets the lifecycle columns with Backlog and Archive; Trash is never part of it
  // (HS2-4J6764).
  if (view === ALL_TICKETS_SCOPE)
    return [
      { id: 'backlog', title: 'Backlog', tickets: tickets.filter((ticket) => ticket.status === 'backlog') },
      ...groups,
      { id: 'archive', title: 'Archive', tickets: tickets.filter(isArchivedTicket) },
    ];
  return groups;
}
