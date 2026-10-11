import { signal } from 'kerfjs';
import { describe, expect, it, vi } from 'vitest';

import type { CheckoutTicketCounts, TicketRow } from '../api';
import type { BoardColumnPage } from '../board-pagination';
import type { WorkspaceViewMode } from '../components/workspace-header';
import type { Project } from '../interactions/types';
import type { TicketView } from '../ticket-views';
import { createBoardColumnPaging } from './board-column-paging';

const row = (slug: string, status: string) => ({ slug, status }) as TicketRow;

function owner(options: { mode?: WorkspaceViewMode; view?: TicketView; search?: boolean } = {}) {
  const current = { id: 'p1', apiPath: '/api/p1' } as Project,
    refreshProject = vi.fn(() => Promise.resolve()),
    loadNextTicketPage = vi.fn(() => Promise.resolve()),
    bindings = {
      viewMode: signal<WorkspaceViewMode>(options.mode ?? 'board'),
      selectedProjectId: signal('p1'),
      selectedView: signal<TicketView>(options.view ?? 'all'),
      hideVerifiedByProject: signal<Record<string, boolean>>({}),
      boardColumnPages: signal<Record<string, BoardColumnPage>>({}),
      boardColumnLoading: signal<Record<string, boolean>>({}),
      tickets: signal<TicketRow[]>([]),
      ticketRowsByProject: signal<Record<string, TicketRow[]>>({}),
      ticketCountsByProject: signal<Record<string, CheckoutTicketCounts>>({}),
      partialSourcesByProject: signal<Record<string, boolean>>({}),
      error: signal(''),
    };
  const paging = createBoardColumnPaging({
    ...bindings,
    project: () => current,
    hideVerifiedColumn: () => false,
    workspaceSearchActive: () => options.search ?? false,
    sortedTicketQuery: (query) => query,
    visibleTickets: () => bindings.tickets.value,
    refreshProject,
    loadNextTicketPage,
    recordAuthoritativeTicketTrend: vi.fn(),
    scheduleClaimLeaseExpiry: vi.fn(),
  });
  return { paging, bindings, refreshProject, loadNextTicketPage };
}

describe('board column paging owner (HS2-H2R2JB)', () => {
  it('builds a per-column refresh spec only for a per-column board, keeping paged column lengths', () => {
    expect(owner({ mode: 'list' }).paging.boardRefreshSpec('all')).toBeUndefined();
    expect(owner({ search: true }).paging.boardRefreshSpec('all')).toBeUndefined();
    const { paging } = owner(),
      rows = [row('A', 'started'), row('B', 'started'), row('C', 'not_started')],
      spec = paging.boardRefreshSpec('all', 'p1', { rows, pages: { started: {} as BoardColumnPage } });
    expect(spec?.columns.map((column) => column.id)).toEqual(['not-started', 'started', 'completed', 'verified']);
    expect(spec?.wants).toEqual({ started: 2 });
    expect(paging.countTicketsForStatus(rows, 'not_started')).toBe(1);
  });

  it('marks only a paged column whose status changed and has more rows as refilling, then clears it', async () => {
    const { paging, bindings, refreshProject } = owner();
    bindings.boardColumnPages.value = { started: {} as BoardColumnPage, completed: {} as BoardColumnPage };
    const before = [row('A', 'started'), row('B', 'started')],
      after = [row('A', 'completed'), row('B', 'started')],
      counts = { started: 30, completed: 1 } as unknown as CheckoutTicketCounts;
    paging.beginBulkBoardRefill('p1', before, after, counts);
    expect(bindings.boardColumnLoading.value).toEqual({ started: true });
    expect(Object.keys(bindings.boardColumnPages.value)).toEqual(['completed']);
    await paging.finishBulkBoardRefill('p1');
    expect(refreshProject).toHaveBeenCalledWith({ showLoading: false });
    expect(bindings.boardColumnLoading.value).toEqual({});
    // Another project, or no change, leaves the state alone.
    paging.beginBulkBoardRefill('p2', before, after, counts);
    paging.beginBulkBoardRefill('p1', before, before, counts);
    expect(bindings.boardColumnLoading.value).toEqual({});
  });

  it('routes a non-status column to the global next page and reveals already-loaded rows first', async () => {
    const { paging, bindings, loadNextTicketPage } = owner();
    await paging.loadBoardColumnMore('unknown-column');
    expect(loadNextTicketPage).toHaveBeenCalledTimes(1);
    bindings.tickets.value = Array.from({ length: 60 }, (_, index) => row(`S${index}`, 'started'));
    bindings.boardColumnPages.value = { started: { loaded: 60, visible: 50 } };
    await paging.loadBoardColumnMore('started');
    expect(bindings.boardColumnPages.value.started.visible).toBe(150);
  });
});
