import { describe, expect, it } from 'vitest';

import type { TicketRow } from './api';
import { ticketBoardGroups } from './ticket-board-layout';

const ticket = (status: string): TicketRow => ({
  connection_id: 'git', native_id: status, qualified_id: `git:${status}`, id: status,
  slug: `HS-${status}`, title: status, status, up_next: false, tags: [], blocked_by: [], claim_count: 0,
});

describe('ticketBoardGroups', () => {
  const queue = ['not_started', 'started', 'completed', 'verified'].map(ticket);

  it('builds one Queue column for each lifecycle status', () => {
    const groups = ticketBoardGroups(queue, 'all', false);
    expect(groups.map(group => group.title)).toEqual(['Not Started', 'Started', 'Completed', 'Verified']);
    expect(groups.map(group => group.tickets.map(item => item.status))).toEqual([
      ['not_started'], ['started'], ['completed'], ['verified'],
    ]);
  });

  it('merges Verified tickets into Completed when its column is hidden', () => {
    const groups = ticketBoardGroups(queue, 'all', true);
    expect(groups.map(group => group.title)).toEqual(['Not Started', 'Started', 'Completed']);
    expect(groups.at(-1)?.tickets.map(item => item.status)).toEqual(['completed', 'verified']);
  });

  it('uses one column for Backlog and Archive views', () => {
    expect(ticketBoardGroups([ticket('backlog')], 'backlog', false)).toMatchObject([
      { id: 'backlog', title: 'Backlog', tickets: [{ status: 'backlog' }] },
    ]);
    expect(ticketBoardGroups([ticket('archive'), ticket('deleted')], 'archive', false)).toMatchObject([
      { id: 'archive', title: 'Archive', tickets: [{ status: 'archive' }, { status: 'deleted' }] },
    ]);
  });
});
