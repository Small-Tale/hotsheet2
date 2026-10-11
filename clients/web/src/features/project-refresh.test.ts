import { signal } from 'kerfjs';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { CheckoutTicketCounts, CorruptTicket, FullTicket, TicketRow } from '../api';
import type { BoardColumnPage } from '../board-pagination';
import type { Project } from '../interactions/types';
import type { ProjectTicketSources } from '../new-ticket-source';
import { PendingCreatedTickets } from '../pending-created-tickets';
import type { ProjectTicketRefresh } from '../project-ticket-refresh';
import { createRefreshBarrier } from '../refresh-barrier';
import type { TicketView } from '../ticket-views';
import { type CachedProjectProjection, createProjectRefresh } from './project-refresh';

const load = vi.hoisted(() => vi.fn<() => Promise<ProjectTicketRefresh>>());
vi.mock('../project-ticket-refresh', async (original) => ({
  ...(await original<typeof import('../project-ticket-refresh')>()),
  loadProjectTicketRefresh: load,
}));

const row = (slug: string) => ({ id: slug, slug, qualified_id: `git:${slug}`, status: 'started' }) as TicketRow;
const counts = { total: 2 } as CheckoutTicketCounts;

function owner() {
  const current = { id: 'p1', apiPath: '/api/p1' } as Project,
    barrier = createRefreshBarrier();
  let anchor: string | undefined = 'GONE';
  const bindings = {
    loading: signal(false),
    error: signal(''),
    selectedView: signal<TicketView>('all'),
    tickets: signal<TicketRow[]>([]),
    ticketRowsByProject: signal<Record<string, TicketRow[]>>({}),
    ticketNextCursor: signal<string | undefined>(undefined),
    ticketCountsByProject: signal<Record<string, CheckoutTicketCounts>>({}),
    partialSourcesByProject: signal<Record<string, boolean>>({}),
    boardColumnPages: signal<Record<string, BoardColumnPage>>({}),
    defaultProviders: signal<Record<string, ProjectTicketSources | undefined>>({
      p1: { sources: [{ provider: 'git' }] } as unknown as ProjectTicketSources,
    }),
    selectedTicketSlugs: signal<string[]>(['A', 'GONE']),
    selectedTicket: signal<FullTicket | null>(null),
    corruptTickets: signal<CorruptTicket[]>([]),
    selectedCorruptKey: signal<string | undefined>(undefined),
    repository: signal(null),
    repositoryError: signal(''),
    commandDefinitions: signal([]),
    commandRuns: signal([]),
    projectProjectionById: signal<Record<string, CachedProjectProjection>>({}),
  };
  const refresh = createProjectRefresh({
    ...bindings,
    localTicketMutationBarrier: barrier,
    pendingCreatedTickets: new PendingCreatedTickets(),
    ticketSelectionAnchor: {
      get: () => anchor,
      set: (value) => {
        anchor = value;
      },
    },
    project: () => current,
    sortedTicketQuery: (query) => query,
    boardRefreshSpec: () => undefined,
    boardPagesWithVisibleLimit: (pages) => pages,
    workspaceSearchActive: () => false,
    refreshTicketSearch: vi.fn(async () => undefined),
    markProjectWarm: vi.fn(),
    recordAuthoritativeTicketTrend: vi.fn(),
    scheduleClaimLeaseExpiry: vi.fn(),
    reconcileRefreshedSelected: vi.fn(),
  });
  return { refresh, bindings, barrier, anchor: () => anchor };
}

beforeEach(() => {
  load.mockReset();
});

describe('selected project refresh owner (HS2-BSSWGV)', () => {
  it('applies the authoritative rows, counts, and projection and prunes a vanished selection', async () => {
    const { refresh, bindings, anchor } = owner();
    load.mockResolvedValue({ tickets: [row('A'), row('B')], ticketCounts: counts, nextCursor: 'c2' });
    await refresh.refreshProject();
    expect(bindings.tickets.value.map((item) => item.slug)).toEqual(['A', 'B']);
    expect(bindings.ticketRowsByProject.value.p1).toHaveLength(2);
    expect(bindings.ticketCountsByProject.value.p1).toBe(counts);
    expect(bindings.ticketNextCursor.value).toBe('c2');
    expect(bindings.selectedTicketSlugs.value).toEqual(['A']);
    expect(anchor()).toBeUndefined();
    expect(bindings.projectProjectionById.value.p1).toMatchObject({ corruptTickets: [], repository: null });
    expect(bindings.loading.value).toBe(false);
  });

  it('drops a refresh invalidated by a local write that began while it waited', async () => {
    const { refresh, bindings, barrier } = owner();
    load.mockResolvedValue({ tickets: [row('STALE')], ticketCounts: counts });
    const release = barrier.begin(),
      pending = refresh.refreshProject();
    refresh.invalidate();
    release();
    await pending;
    expect(load).not.toHaveBeenCalled();
    expect(bindings.tickets.value).toEqual([]);
  });

  it('reports a failed refresh as the workspace error', async () => {
    const { refresh, bindings } = owner();
    load.mockRejectedValue(new Error('offline'));
    await refresh.refreshProject();
    expect(bindings.error.value).toBe('offline');
    expect(bindings.loading.value).toBe(false);
  });
});
