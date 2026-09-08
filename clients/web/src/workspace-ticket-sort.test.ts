import { describe, expect, it } from 'vitest';

import { compareWorkspaceTickets, type WorkspaceSortableTicket } from './workspace-ticket-sort';

const ticket = (slug: string, status: string, priority: string): WorkspaceSortableTicket => ({ slug, title: slug, status, priority });

describe('workspace ticket sorting', () => {
  it('uses workflow status order rather than alphabetical wire values', () => {
    const rows = [ticket('verified', 'verified', 'default'), ticket('started', 'started', 'default'), ticket('archive', 'archive', 'default'), ticket('backlog', 'backlog', 'default'), ticket('completed', 'completed', 'default'), ticket('not-started', 'not_started', 'default')];
    expect(rows.sort((a, b) => compareWorkspaceTickets(a, b, 'status', 'ascending')).map(row => row.status)).toEqual(['backlog', 'not_started', 'started', 'completed', 'verified', 'archive']);
  });

  it('sorts ascending priority from low through urgent and reverses exactly', () => {
    const rows = [ticket('urgent', 'started', 'urgent'), ticket('default', 'started', 'default'), ticket('low', 'started', 'low'), ticket('high', 'started', 'high')];
    expect([...rows].sort((a, b) => compareWorkspaceTickets(a, b, 'priority', 'ascending')).map(row => row.priority)).toEqual(['low', 'default', 'high', 'urgent']);
    expect([...rows].sort((a, b) => compareWorkspaceTickets(a, b, 'priority', 'descending')).map(row => row.priority)).toEqual(['urgent', 'high', 'default', 'low']);
  });
});
