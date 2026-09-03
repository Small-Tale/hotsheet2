import { describe, expect, it } from 'vitest';

import type { TicketRow } from './api';
import { canCreateTicketInView, isArchivedTicket, isOpenTicket, isQueuedTicket, isUpNextTicket, newTicketStatusForView, ticketsForView } from './ticket-views';

const ticket = (status: string): TicketRow => ({
  connection_id: 'git', native_id: status, qualified_id: `git:${status}`, id: status,
  slug: `HS-${status}`, title: status, status, up_next: false, feedback_needed: false, tags: [], blocked_by: [], claim_count: 0,
});

describe('ticket views', () => {
  it('partitions active queue, backlog, and every archived status without overlap', () => {
    const tickets = ['not_started', 'started', 'backlog', 'completed', 'verified', 'archive', 'deleted', 'moved'].map(ticket);
    expect(tickets.filter(isQueuedTicket).map(item => item.status)).toEqual(['not_started', 'started', 'completed', 'verified']);
    expect(tickets.filter(isArchivedTicket).map(item => item.status)).toEqual(['archive', 'deleted', 'moved']);
    expect(ticketsForView(tickets, 'all').map(item => item.status)).toEqual(['not_started', 'started', 'completed', 'verified']);
    expect(ticketsForView(tickets, 'backlog').map(item => item.status)).toEqual(['backlog']);
    expect(ticketsForView(tickets, 'archive').map(item => item.status)).toEqual(['archive', 'deleted', 'moved']);
    expect(ticketsForView(tickets, 'errors')).toEqual([]);
  });

  it('creates into the visible active destination and disables creation for Archive', () => {
    expect(newTicketStatusForView('all')).toBe('not_started');
    expect(newTicketStatusForView('backlog')).toBe('backlog');
    expect(canCreateTicketInView('all')).toBe(true);
    expect(canCreateTicketInView('backlog')).toBe(true);
    expect(canCreateTicketInView('archive')).toBe(false);
    expect(canCreateTicketInView('errors')).toBe(false);
  });

  it('derives open and Up Next summary counts from workflow semantics', () => {
    const tickets = ['not_started', 'started', 'backlog', 'completed', 'verified', 'archive', 'deleted', 'moved'].map((status, index) => ({ ...ticket(status), up_next: index !== 1 }));
    expect(tickets.filter(isOpenTicket).map(item => item.status)).toEqual(['not_started', 'started', 'backlog']);
    expect(tickets.filter(isUpNextTicket).map(item => item.status)).toEqual(['not_started', 'backlog']);
  });
});
