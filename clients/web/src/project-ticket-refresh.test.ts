import { describe, expect, it, vi } from 'vitest';

import type { Api } from './api';
import {
  appendUniqueTicketRows,
  BOARD_COLUMN_PAGE_SIZE,
  loadBoardColumnRefresh,
  loadProjectTicketRefresh,
} from './project-ticket-refresh';

describe('loadProjectTicketRefresh', () => {
  const counts = {
    total: 1,
    queued: 1,
    backlog: 0,
    archive: 0,
    open: 1,
    up_next: 0,
    active: 0,
    started: 0,
    completed_today: 0,
  };
  it('accepts the legacy ticket-array response while compatible servers transition to pagination', async () => {
    const ticket = { id: '1', slug: 'HS2-LEGACY', title: 'Legacy', tags: [] };
    const result = await loadProjectTicketRefresh(
      {
        checkoutTicketPage: vi.fn().mockResolvedValue([ticket]),
        checkoutCorruptTickets: vi.fn().mockResolvedValue([]),
      },
      'checkout',
    );
    expect(result).toEqual({ tickets: [ticket], corruptTickets: [] });
  });
  it('keeps healthy tickets when the corrupt-ticket index fails', async () => {
    const ticket = { id: '01', slug: 'HS2-OK', title: 'Healthy', tags: [] };
    const checkoutTicketPage = vi.fn().mockResolvedValue({ items: [ticket], counts });
    const result = await loadProjectTicketRefresh(
      {
        checkoutTicketPage,
        checkoutCorruptTickets: vi.fn().mockRejectedValue(new Error('index unavailable')),
      },
      'checkout',
    );

    expect(result).toEqual({
      tickets: [ticket],
      ticketCounts: counts,
      nextCursor: undefined,
      corruptTicketsError: 'index unavailable',
    });
    expect(checkoutTicketPage).toHaveBeenCalledWith('checkout', 200, undefined, { collection: 'queue' });
  });

  it('keeps corrupt entries available when the healthy-ticket index fails', async () => {
    const corrupt = {
      store: 'local',
      store_path: '/tickets',
      path: '/tickets/01.md',
      slug: 'HS2-BAD',
      error: 'invalid notes',
    };
    const result = await loadProjectTicketRefresh(
      {
        checkoutTicketPage: vi.fn().mockRejectedValue(new Error('healthy index unavailable')),
        checkoutCorruptTickets: vi.fn().mockResolvedValue([corrupt]),
      },
      'checkout',
    );

    expect(result).toEqual({ ticketsError: 'healthy index unavailable', corruptTickets: [corrupt] });
  });

  it('removes a stale healthy row when live diagnostics identify the same slug as corrupt', async () => {
    const healthy = { id: '01', slug: 'HS2-OK', title: 'Healthy', tags: [] };
    const stale = { id: '02', slug: 'HS2-BAD', title: 'Stale indexed title', tags: [] };
    const corrupt = {
      store: 'local',
      store_path: '/tickets',
      path: '/tickets/02.md',
      slug: 'HS2-BAD',
      error: 'invalid notes',
    };
    const result = await loadProjectTicketRefresh(
      {
        checkoutTicketPage: vi.fn().mockResolvedValue({ items: [healthy, stale], counts, next_cursor: 'next' }),
        checkoutCorruptTickets: vi.fn().mockResolvedValue([corrupt]),
      },
      'checkout',
    );

    expect(result).toEqual({ tickets: [healthy], ticketCounts: counts, nextCursor: 'next', corruptTickets: [corrupt] });
  });
});

describe('appendUniqueTicketRows', () => {
  const ticket = (connection_id: string, native_id: string) => ({
    connection_id,
    native_id,
    qualified_id: `${connection_id}:${native_id}`,
    id: native_id,
    slug: `HS2-${native_id}`,
    title: native_id,
    up_next: false,
    feedback_needed: false,
    tags: [],
    blocked_by: [],
    claim_count: 0,
  });
  it('appends a continuation page without duplicating overlapping or repeated rows', () => {
    const first = ticket('local', '1'),
      second = ticket('local', '2');
    expect(appendUniqueTicketRows([first], [first, second, second])).toEqual([first, second]);
  });
});

describe('loadBoardColumnRefresh (HS2-HNZZHC)', () => {
  const counts = {
    total: 0,
    queued: 0,
    backlog: 0,
    archive: 0,
    open: 0,
    up_next: 0,
    active: 0,
    started: 0,
    completed_today: 0,
  };
  const rows = (status: string, total: number) =>
    Array.from({ length: total }, (_, index) => ({
      id: `${status}-${index}`,
      slug: `HS2-${status}-${index}`,
      title: `${status} ${index}`,
      tags: [],
      status,
      connection_id: 'git',
      native_id: `${status}-${index}`,
    }));
  /** A fake checkout: each status stream pages its rows by numeric cursor offset. */
  function fakeClient(data: Record<string, ReturnType<typeof rows>>) {
    const page = (size: number, cursor: string | undefined, query: { status?: string }) => {
      const all = data[query.status ?? ''] ?? [],
        start = cursor ? Number(cursor) : 0,
        end = start + size;
      return { items: all.slice(start, end), ...(end < all.length ? { next_cursor: String(end) } : {}) };
    };
    const checkoutTicketPage = vi.fn(
        (_checkout: string, size: number, cursor: string | undefined, query: { status?: string }) =>
          Promise.resolve({ ...page(size, cursor, query), counts }),
      ),
      checkoutTicketRowsPage = vi.fn(
        (_checkout: string, size: number, cursor: string | undefined, query: { status?: string }) =>
          Promise.resolve({ ...page(size, cursor, query), counts: null }),
      );
    const mocks = { checkoutTicketPage, checkoutTicketRowsPage };
    // The fake serves the real wire shape for the fields the loader reads; the intersection keeps mock access.
    return mocks as typeof mocks & Pick<Api, 'checkoutTicketPage' | 'checkoutTicketRowsPage'>;
  }
  const columns = [
    { id: 'not-started', statuses: ['not_started'] },
    { id: 'started', statuses: ['started'] },
    { id: 'completed', statuses: ['completed'] },
    { id: 'verified', statuses: ['verified'] },
  ];

  it('loads every column independently so a short column is never starved by a long one', async () => {
    const client = fakeClient({
      not_started: rows('not_started', 75),
      started: rows('started', 4),
      completed: rows('completed', 169),
      verified: rows('verified', 2),
    });
    const result = await loadBoardColumnRefresh(client, 'demo', { collection: 'queue', sort: 'updated' }, columns);
    const byStatus = (status: string) => result.tickets.filter((ticket) => ticket.status === status).length;
    expect(byStatus('not_started')).toBe(75);
    expect(byStatus('started')).toBe(4);
    expect(byStatus('completed')).toBe(100);
    expect(byStatus('verified')).toBe(2);
    expect(result.pages.started).toEqual({ loaded: 4, exhausted: true, streams: { started: { exhausted: true } } });
    expect(result.pages.completed).toEqual({
      loaded: 100,
      exhausted: false,
      streams: { completed: { cursor: '100', exhausted: false } },
    });
    // Exactly one request carries counts; every request pages 100 rows in the view's sort.
    expect(result.counts).toBe(counts);
    expect(client.checkoutTicketPage).toHaveBeenCalledTimes(1);
    expect(client.checkoutTicketRowsPage).toHaveBeenCalledTimes(3);
    for (const call of [...client.checkoutTicketPage.mock.calls, ...client.checkoutTicketRowsPage.mock.calls]) {
      expect(call[1]).toBe(BOARD_COLUMN_PAGE_SIZE);
      expect(call[3]).toMatchObject({ collection: 'queue', sort: 'updated' });
    }
  });

  it('restores an expanded column to its loaded length in pages of at most 500', async () => {
    const client = fakeClient({ completed: rows('completed', 900), not_started: rows('not_started', 3) });
    const result = await loadBoardColumnRefresh(client, 'demo', {}, columns.slice(0, 3), { completed: 620 });
    expect(result.tickets.filter((ticket) => ticket.status === 'completed')).toHaveLength(620);
    expect(result.pages.completed.streams?.completed).toEqual({ cursor: '620', exhausted: false });
    const completedSizes = [...client.checkoutTicketPage.mock.calls, ...client.checkoutTicketRowsPage.mock.calls]
      .filter((call) => call[3].status === 'completed')
      .map((call) => call[1]);
    expect(completedSizes).toEqual([500, 120]);
    // A column below one page still loads a full first page, not its smaller previous length.
    const shrunk = await loadBoardColumnRefresh(client, 'demo', {}, columns.slice(2, 3), { completed: 30 });
    expect(shrunk.tickets).toHaveLength(100);
  });

  it('pages a merged Completed column into Verified only after Completed is exhausted', async () => {
    const merged = [{ id: 'completed', statuses: ['completed', 'verified'] }];
    const short = await loadBoardColumnRefresh(
      fakeClient({ completed: rows('completed', 30), verified: rows('verified', 90) }),
      'demo',
      {},
      merged,
    );
    expect(short.tickets.filter((ticket) => ticket.status === 'verified')).toHaveLength(70);
    expect(short.pages.completed).toEqual({
      loaded: 100,
      exhausted: false,
      streams: { completed: { exhausted: true }, verified: { cursor: '70', exhausted: false } },
    });
    const long = await loadBoardColumnRefresh(
      fakeClient({ completed: rows('completed', 150), verified: rows('verified', 5) }),
      'demo',
      {},
      merged,
    );
    expect(long.tickets.every((ticket) => ticket.status === 'completed')).toBe(true);
    expect(long.pages.completed.streams).toEqual({ completed: { cursor: '100', exhausted: false } });
    // Verified rows the column already showed are restored even while Completed has more.
    const restored = await loadBoardColumnRefresh(
      fakeClient({ completed: rows('completed', 150), verified: rows('verified', 5) }),
      'demo',
      {},
      merged,
      { verified: 5 },
    );
    expect(restored.tickets.filter((ticket) => ticket.status === 'verified')).toHaveLength(5);
  });

  it('accepts a legacy row-array server as one exhausted page per column', async () => {
    const legacy = [...rows('not_started', 2), ...rows('started', 1)];
    const result = await loadBoardColumnRefresh(
      {
        checkoutTicketPage: vi.fn().mockResolvedValue(legacy),
        checkoutTicketRowsPage: vi.fn().mockResolvedValue(legacy),
      },
      'demo',
      {},
      columns.slice(0, 3),
    );
    expect(result.tickets.map((ticket) => ticket.slug)).toEqual(legacy.map((ticket) => ticket.slug));
    expect(result.counts).toBeUndefined();
    expect(result.pages['not-started']).toEqual({
      loaded: 3,
      exhausted: true,
      streams: { not_started: { exhausted: true } },
    });
    expect(result.pages.completed.exhausted).toBe(true);
  });

  it('empty columns are exhausted and a board refresh carries pages, counts, and no global cursor', async () => {
    const client = {
      ...fakeClient({ started: rows('started', 1) }),
      checkoutCorruptTickets: vi.fn().mockResolvedValue([]),
    };
    const result = await loadProjectTicketRefresh(client, 'demo', { collection: 'queue' }, { columns });
    expect(result.tickets).toHaveLength(1);
    expect(result.nextCursor).toBeUndefined();
    expect(result.ticketCounts).toBe(counts);
    expect(result.boardPages?.['not-started']).toEqual({
      loaded: 0,
      exhausted: true,
      streams: { not_started: { exhausted: true } },
    });
    // Without the counts-free endpoint every column falls back to counted pages.
    const legacy = fakeClient({ started: rows('started', 1) });
    const fallback = await loadBoardColumnRefresh(
      { checkoutTicketPage: legacy.checkoutTicketPage },
      'demo',
      {},
      columns,
    );
    expect(fallback.counts).toBe(counts);
    expect(legacy.checkoutTicketPage).toHaveBeenCalledTimes(4);
  });
});
