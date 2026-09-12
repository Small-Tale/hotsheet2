import { describe, expect, it } from 'vitest';

import type { TicketRow } from './api';
import { ticketBoardGroups,ticketBoardGroupTotal } from './ticket-board-layout';

const ticket = (status: string): TicketRow => ({
  connection_id: 'git', native_id: status, qualified_id: `git:${status}`, id: status,
  slug: `HS-${status}`, title: status, status, up_next: false, feedback_needed: false, tags: [], blocked_by: [], claim_count: 0,
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

  it('keeps searched Queue results in Queue columns', () => {
    const results = [...queue, 'backlog', 'archive', 'deleted', 'moved'].map(status =>
      typeof status === 'string' ? ticket(status) : status,
    );
    const groups = ticketBoardGroups(results, 'all', false);
    expect(groups.map(group => group.title)).toEqual(['Not Started', 'Started', 'Completed', 'Verified']);
    expect(groups.flatMap(group => group.tickets)).toHaveLength(queue.length);
  });

  it('derives absolute built-in column totals from the checkout summary',()=>{
    const counts={total:390,queued:350,backlog:25,archive:15,open:270,up_next:4,active:1,started:120,verified:30,completed_today:2};
    expect(['not-started','started','completed','verified'].map(id=>ticketBoardGroupTotal(id,10,'all',counts,false))).toEqual([150,120,50,30]);
    expect(ticketBoardGroupTotal('completed',10,'all',counts,true)).toBe(80);
    expect(ticketBoardGroupTotal('backlog',10,'backlog',counts,false)).toBe(25);
    expect(ticketBoardGroupTotal('archive',10,'archive',counts,false)).toBe(15);
    expect(ticketBoardGroupTotal('completed',10,'all',{...counts,verified:undefined},false)).toBe(10);
    expect(ticketBoardGroupTotal('started',10,'custom:mine',counts,false)).toBe(10);
  });
});
