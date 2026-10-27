import { signal } from 'kerfjs';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { CheckoutTicketCounts, CheckoutTicketQuery, CorruptTicket, FullTicket, TicketRow } from '../api';
import type { BoardColumnPage } from '../board-pagination';
import type { Project } from '../interactions/types';
import type { ProjectTicketSources } from '../new-ticket-source';
import { PendingCreatedTickets } from '../pending-created-tickets';
import type { ProjectTicketRefresh } from '../project-ticket-refresh';
import { createRefreshBarrier } from '../refresh-barrier';
import type { TicketView } from '../ticket-views';
import { type CachedProjectProjection, createProjectRefresh, sameTicketQuery } from './project-refresh';

const load = vi.hoisted(() => vi.fn<() => Promise<ProjectTicketRefresh>>());
vi.mock('../project-ticket-refresh', async (original) => ({
  ...(await original<typeof import('../project-ticket-refresh')>()),
  loadProjectTicketRefresh: load,
}));

const row = (slug: string) => ({ id: slug, slug, qualified_id: `git:${slug}`, status: 'started' }) as TicketRow;
const counts = { total: 2 } as CheckoutTicketCounts;

function owner() {
  let current = { id: 'p1', apiPath: '/api/p1' } as Project;
  const barrier = createRefreshBarrier();
  let anchor: string | undefined = 'GONE';
  const bindings = {
    loading: signal(false),
    error: signal(''),
    refreshError: signal(''),
    selectedView: signal<TicketView>('all'),
    tickets: signal<TicketRow[]>([]),
    ticketRowsByProject: signal<Record<string, TicketRow[]>>({}),
    ticketNextCursor: signal<string | undefined>(undefined),
    ticketPageQuery: signal<CheckoutTicketQuery>({ collection: 'queue' }),
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
    sortedTicketQuery: (query) => ({ ...query, sort: 'updated', direction: 'descending' }),
    boardRefreshSpec: () => undefined,
    boardPagesWithVisibleLimit: (pages) => pages,
    workspaceSearchActive: () => false,
    refreshTicketSearch: vi.fn(async () => undefined),
    markProjectWarm: vi.fn(),
    recordAuthoritativeTicketTrend: vi.fn(),
    scheduleClaimLeaseExpiry: vi.fn(),
    reconcileRefreshedSelected: vi.fn(),
  });
  return {
    refresh,
    bindings,
    barrier,
    anchor: () => anchor,
    switchProject: () => {
      current = { id: 'p2', apiPath: '/api/p2' } as Project;
      bindings.error.value = '';
      bindings.refreshError.value = '';
    },
  };
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

  it('pairs the continuation cursor with the sorted query it was issued for (HS2-9ZZ8QQ)', async () => {
    const { refresh, bindings } = owner();
    load.mockResolvedValue({ tickets: [row('A')], ticketCounts: counts, nextCursor: 'c-sorted' });
    await refresh.refreshProject();
    const requested = load.mock.calls[0] as unknown as [unknown, string, CheckoutTicketQuery];
    expect(requested[2]).toMatchObject({ sort: 'updated', direction: 'descending' });
    expect(bindings.ticketNextCursor.value).toBe('c-sorted');
    expect(bindings.ticketPageQuery.value).toEqual(requested[2]);
    // A later sort change refreshes again: cursor and query move together.
    load.mockResolvedValue({ tickets: [row('B')], ticketCounts: counts, nextCursor: 'c-next' });
    await refresh.refreshProject();
    expect(bindings.ticketPageQuery.value).toEqual((load.mock.calls[1] as unknown as unknown[])[2]);
    expect(bindings.ticketNextCursor.value).toBe('c-next');
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
    expect(bindings.refreshError.value).toBe('offline');
    expect(bindings.error.value).toBe('');
    expect(bindings.loading.value).toBe(false);
  });

  it('HS2-4Q44N2 preserves mutation failures while repeated refreshes replace and clear only index errors', async () => {
    const { refresh, bindings } = owner();
    bindings.error.value = 'ticket changed since it was read';
    load.mockResolvedValueOnce({ tickets: [row('A')], ticketsError: 'index unavailable' });
    await refresh.refreshProject();
    expect(bindings.error.value).toBe('ticket changed since it was read');
    expect(bindings.refreshError.value).toBe('index unavailable');

    load.mockResolvedValueOnce({ tickets: [row('A')], corruptTicketsError: 'corrupt source unavailable' });
    await refresh.refreshProject();
    expect(bindings.error.value).toBe('ticket changed since it was read');
    expect(bindings.refreshError.value).toBe('corrupt source unavailable');

    load.mockResolvedValueOnce({ tickets: [row('A')] });
    await refresh.refreshProject();
    expect(bindings.error.value).toBe('ticket changed since it was read');
    expect(bindings.refreshError.value).toBe('');
  });

  it('HS2-4Q44N2 keeps equal-text mutation and refresh failures independent through dismissal and reset', async () => {
    const { refresh, bindings } = owner();
    bindings.error.value = 'same failure';
    load.mockResolvedValueOnce({ tickets: [row('A')], ticketsError: 'same failure' });
    await refresh.refreshProject();
    expect(bindings.error.value).toBe('same failure');
    expect(bindings.refreshError.value).toBe('same failure');

    // Dismissal and a further edit clear/change the mutation owner independently.
    bindings.error.value = '';
    bindings.refreshError.value = '';
    bindings.error.value = 'next edit failed';
    load.mockResolvedValueOnce({ tickets: [row('A')] });
    await refresh.refreshProject();
    expect(bindings.error.value).toBe('next edit failed');
    expect(bindings.refreshError.value).toBe('');
  });

  it('HS2-4Q44N2 ignores an old project refresh after activation resets both error owners', async () => {
    const { refresh, bindings, switchProject } = owner();
    let finish: ((value: ProjectTicketRefresh) => void) | undefined;
    load.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    bindings.error.value = 'old mutation';
    const pending = refresh.refreshProject();
    await vi.waitFor(() => {
      expect(finish).toBeDefined();
    });
    switchProject();
    finish?.({ tickets: [row('OLD')], ticketsError: 'old source' });
    await pending;
    expect(bindings.error.value).toBe('');
    expect(bindings.refreshError.value).toBe('');
  });
});

describe('sameTicketQuery (HS2-9DF0DA)', () => {
  it('matches queries that select the same rows in the same order', () => {
    expect(
      sameTicketQuery({ collection: 'archive', sort: 'updated' }, { sort: 'updated', collection: 'archive' }),
    ).toBe(true);
    expect(sameTicketQuery({ collection: 'archive' }, { collection: 'archive', tags: undefined })).toBe(true);
    expect(sameTicketQuery({ collection: 'archive' }, { collection: 'queue' })).toBe(false);
    expect(sameTicketQuery({ collection: 'archive', sort: 'updated' }, { collection: 'archive', sort: 'title' })).toBe(
      false,
    );
    expect(sameTicketQuery({ tags: 'a,b' }, { tags: 'b,a' })).toBe(false);
    expect(sameTicketQuery({}, {})).toBe(true);
  });
});
