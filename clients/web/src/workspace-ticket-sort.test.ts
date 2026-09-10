import { describe, expect, it } from 'vitest';

import { compareWorkspaceTickets, type WorkspaceSortableTicket } from './workspace-ticket-sort';

const ticket = (slug: string, status: string, priority: string): WorkspaceSortableTicket => ({ slug, title: slug, status, priority });
const updatedTicket = (slug: string, updated_at: string, overrides: Partial<WorkspaceSortableTicket> = {}): WorkspaceSortableTicket => ({ slug, title: slug, status: 'started', priority: 'default', updated_at, ...overrides });

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

  it.each([
    ['status', { status: 'started' }],
    ['priority', { priority: 'high' }],
    ['title', { title: 'Same title' }],
  ] as const)('uses recently updated as the secondary order for %s in either direction', (sort, shared) => {
    const rows = [
      updatedTicket('oldest', '2026-09-08T10:00:00Z', shared),
      updatedTicket('newest', '2026-09-10T10:00:00Z', shared),
      updatedTicket('middle', '2026-09-09T10:00:00Z', shared),
    ];
    expect([...rows].sort((a, b) => compareWorkspaceTickets(a, b, sort, 'ascending')).map(row => row.slug)).toEqual(['newest', 'middle', 'oldest']);
    expect([...rows].sort((a, b) => compareWorkspaceTickets(a, b, sort, 'descending')).map(row => row.slug)).toEqual(['newest', 'middle', 'oldest']);
  });

  it('keeps the selected direction primary before applying the recent-first tie break', () => {
    const rows = [
      updatedTicket('older-high', '2026-09-08T10:00:00Z', { priority: 'high' }),
      updatedTicket('newer-low', '2026-09-10T10:00:00Z', { priority: 'low' }),
    ];
    expect([...rows].sort((a, b) => compareWorkspaceTickets(a, b, 'priority', 'ascending')).map(row => row.slug)).toEqual(['newer-low', 'older-high']);
    expect([...rows].sort((a, b) => compareWorkspaceTickets(a, b, 'priority', 'descending')).map(row => row.slug)).toEqual(['older-high', 'newer-low']);
  });
});
