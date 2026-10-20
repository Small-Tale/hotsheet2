import { readFileSync } from 'node:fs';

import Ajv from 'ajv';
import { signal } from 'kerfjs';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { filterAdvancedSearchResults } from './advanced-search';
import type { CheckoutTicketQuery, CustomView, PollResponse, TicketRow } from './api';
import { selectAllTickets, type TicketSelectionState, updateTicketSelection } from './components/ticket-selection';
import { createWorkspaceSearchController, type SidebarSearchCounts } from './features/workspace-search';
import type { InlineSearchToken } from './inline-search';
import type { Project } from './interactions/types';
import { startProjectChangeStream } from './project-change-poll';
import { projectTicketPatch } from './ticket-mutation';
import { TicketHistory, type TicketPatch } from './ticket-operations';
import { selectionAfterTicketViewChange, ticketsForView, type TicketView } from './ticket-views';
import { compareWorkspaceTickets } from './workspace-ticket-sort';

const location = (name: string) => new URL(`../../../docs/contracts/v1/client-state/${name}.json`, import.meta.url);
const validateFixture = new Ajv({ allErrors: true }).compile(
  JSON.parse(readFileSync(location('schema'), 'utf8')) as object,
);
function fixture(name: string): unknown {
  const value = JSON.parse(readFileSync(location(name), 'utf8')) as { format: string };
  expect(value.format).toBe('hotsheet-client-state/v1');
  expect(validateFixture(value), JSON.stringify(validateFixture.errors)).toBe(true);
  return value;
}

interface RowInput {
  slug: string;
  title: string;
  status?: string;
  priority?: string;
  updated_at?: string;
  up_next?: boolean;
}
const row = (input: RowInput): TicketRow => ({
  connection_id: 'local',
  native_id: input.slug,
  qualified_id: `local:${input.slug}`,
  id: input.slug,
  slug: input.slug,
  title: input.title,
  status: input.status ?? 'not_started',
  priority: input.priority ?? 'default',
  updated_at: input.updated_at,
  up_next: input.up_next ?? false,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
});

interface CollectionFixture {
  initial: {
    rows: RowInput[];
    view: TicketView;
    sort: 'priority' | 'status' | 'title' | 'updated';
    direction: 'ascending' | 'descending';
  };
  steps: Array<{
    action: 'snapshot' | 'select' | 'select_all' | 'view' | 'sort' | 'search';
    slug?: string;
    range?: boolean;
    toggle?: boolean;
    value?: string;
    direction?: 'ascending' | 'descending';
    expect: { visible: string[]; selected: string[]; anchor?: string };
  }>;
}

interface HistoryFixture {
  initial: { slug: string; title: string; status: string; priority: string };
  steps: Array<{
    action: 'begin_edit' | 'ack' | 'remote' | 'undo' | 'redo' | 'queue_undo';
    patch?: TicketPatch;
    expect: { title: string; status: string; priority: string };
  }>;
}

interface SearchFixture {
  initial: { views: Array<{ id: string; query: string }> };
  steps: Array<{
    action: 'request' | 'resolve';
    view: string;
    rows?: RowInput[];
    expect: { visible: string[]; selected_view: string };
  }>;
}

interface ReplayFixture {
  steps: Array<{
    response?: PollResponse;
    error?: string;
    expect: { poll_since: Array<number | null>; resync: string[]; events: string[]; refreshes: number };
  }>;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('portable client-state v1 fixtures (HS2-H6RRZN)', () => {
  it('rejects a transition whose required input is missing', () => {
    const invalid = structuredClone(fixture('collections')) as { steps: Array<Record<string, unknown>> };
    const selection = invalid.steps.find((step) => step.action === 'select')!;
    delete selection.slug;
    expect(validateFixture(invalid)).toBe(false);
  });

  it('runs collection filtering, ordering, selection, range, toggle, and reset through the web model', () => {
    const data = fixture('collections') as CollectionFixture;
    const rows = data.initial.rows.map(row);
    let view = data.initial.view,
      sort = data.initial.sort,
      direction = data.initial.direction,
      search = '',
      selection: TicketSelectionState = { selected: new Set() };
    const visible = () =>
      filterAdvancedSearchResults(ticketsForView(rows, view), search)
        .sort((a, b) => compareWorkspaceTickets(a, b, sort, direction))
        .map((ticket) => ticket.slug);
    for (const [index, step] of data.steps.entries()) {
      if (step.action === 'select') selection = updateTicketSelection(visible(), selection, step.slug!, step);
      else if (step.action === 'select_all') selection = selectAllTickets(visible());
      else if (step.action === 'view') {
        const next = step.value as TicketView;
        selection = { selected: new Set(selectionAfterTicketViewChange(view, next, [...selection.selected])) };
        view = next;
      } else if (step.action === 'sort') {
        sort = step.value as typeof sort;
        direction = step.direction!;
      } else if (step.action === 'search') search = step.value!;
      const order = visible();
      expect(
        {
          visible: order,
          selected: order.filter((slug) => selection.selected.has(slug)),
          ...(selection.anchor === undefined ? {} : { anchor: selection.anchor }),
        },
        `step ${index + 1}: ${step.action}`,
      ).toEqual(step.expect);
    }
  });

  it('runs optimistic edits and field-aware undo/redo across a remote write', async () => {
    const data = fixture('history') as HistoryFixture;
    let ticket = row(data.initial),
      pending: ReturnType<typeof deferred<boolean>> | undefined,
      edit: Promise<boolean> | undefined,
      queuedUndo: Promise<boolean> | undefined;
    const history = new TicketHistory(
      () => ({ slug: ticket.slug, title: ticket.title, status: ticket.status, priority: ticket.priority }),
      async (_slug, patch) => {
        ticket = projectTicketPatch(ticket, patch);
        return pending ? pending.promise : true;
      },
    );
    for (const [index, step] of data.steps.entries()) {
      if (step.action === 'begin_edit') {
        pending = deferred<boolean>();
        edit = history.execute(ticket.slug, step.patch!);
      } else if (step.action === 'ack') {
        pending!.resolve(true);
        pending = undefined;
        expect(await edit).toBe(true);
        if (queuedUndo) {
          expect(await queuedUndo).toBe(true);
          queuedUndo = undefined;
        }
      } else if (step.action === 'remote') ticket = projectTicketPatch(ticket, step.patch!);
      else if (step.action === 'undo') expect(await history.undo()).toBe(true);
      else if (step.action === 'redo') expect(await history.redo()).toBe(true);
      else queuedUndo = history.undo();
      expect(
        { title: ticket.title, status: ticket.status, priority: ticket.priority },
        `step ${index + 1}: ${step.action}`,
      ).toEqual(step.expect);
    }
  });

  it('rejects an older search response after the selected view changes', async () => {
    const data = fixture('stale-search') as SearchFixture,
      selectedView = signal<TicketView>('all'),
      searchMatchKeys = signal<Set<string> | undefined>(undefined),
      tickets = signal<TicketRow[]>([]),
      requests = new Map<string, ReturnType<typeof deferred<Response>>>(),
      active = new Map<string, Promise<void>>(),
      views = new Map(data.initial.views.map((view) => [view.id, view.query]));
    vi.stubGlobal('window', { setTimeout, clearTimeout });
    vi.stubGlobal('fetch', (url: string | URL | Request) => {
      const href = url instanceof Request ? url.url : String(url),
        query = new URL(href, 'http://local').searchParams.get('text') ?? '',
        request = deferred<Response>();
      requests.set(query, request);
      return request.promise;
    });
    const project: Project = {
      id: 'fixture',
      name: 'fixture',
      root: '/fixture',
      apiPath: '/api/fixture',
      stores: [],
      compatibility: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
    };
    const owner = createWorkspaceSearchController({
      searchQuery: signal(''),
      searchTokens: signal<InlineSearchToken[]>([]),
      searchHelpOpen: signal(false),
      searchMatchKeys,
      sidebarSearchCounts: signal<SidebarSearchCounts | undefined>(undefined),
      selectedView,
      searchAllTickets: signal(false),
      error: signal(''),
      ticketPageQuery: signal<CheckoutTicketQuery>({}),
      ticketNextCursor: signal<string | undefined>(undefined),
      tickets,
      ticketRowsByProject: signal({}),
      project: () => project,
      customViewFor: (view) => {
        const id = view.startsWith('custom:') ? view.slice(7) : '';
        return views.has(id) ? ({ id, name: id, query: views.get(id)! } satisfies CustomView) : undefined;
      },
      customViewsFor: () => [],
      activeWorkspaceSort: () => ({ sort: 'priority', sortDirection: 'descending' }),
      ticketSearchKey: (ticket) => `${ticket.connection_id}:${ticket.native_id}`,
      mergeTicketLinkRows: (_existing, incoming) => [...incoming],
      refreshProject: async () => undefined,
      resetBoardColumnPages: () => undefined,
      resetProgressiveTicketRendering: () => undefined,
      scheduleProjectSessionPersistence: () => undefined,
    });
    for (const [index, step] of data.steps.entries()) {
      if (step.action === 'request') {
        selectedView.value = `custom:${step.view}`;
        searchMatchKeys.value = undefined;
        active.set(step.view, owner.refreshTicketSearch());
      } else {
        const query = views.get(step.view)!;
        requests
          .get(query)!
          .resolve(
            Response.json({ items: step.rows!.map(row), next_cursor: null, counts: { total: step.rows!.length } }),
          );
        await active.get(step.view);
      }
      expect(
        {
          visible: tickets.value
            .filter((ticket) => searchMatchKeys.value?.has(`${ticket.connection_id}:${ticket.native_id}`))
            .map((ticket) => ticket.slug),
          selected_view: selectedView.value.slice(7),
        },
        `step ${index + 1}: ${step.action}`,
      ).toEqual(step.expect);
    }
  });

  it('replays ticket events through the shipped long-poll fallback and reconnects after failure', async () => {
    const data = fixture('replay') as ReplayFixture,
      polls: Array<{ since: number | undefined; next: ReturnType<typeof deferred<PollResponse>> }> = [],
      resync: string[] = [],
      events: string[] = [];
    let refreshes = 0;
    const stop = startProjectChangeStream({
      client: {
        pollEvents: (since) => {
          const next = deferred<PollResponse>();
          polls.push({ since, next });
          return next.promise;
        },
      },
      refresh: async () => {
        refreshes += 1;
      },
      onEvents: async (page) => {
        events.push(...page.events.map((event) => event.kind));
      },
      onResync: (reason) => {
        resync.push(reason);
      },
      wait: async () => undefined,
    });
    try {
      for (const [index, step] of data.steps.entries()) {
        await vi.waitFor(() => {
          expect(polls).toHaveLength(index + 1);
        });
        if (step.error) polls[index].next.reject(new Error(step.error));
        else polls[index].next.resolve(step.response!);
        await vi.waitFor(() => {
          expect(polls).toHaveLength(index + 2);
        });
        expect(
          { poll_since: polls.map((poll) => poll.since ?? null), resync, events, refreshes },
          `step ${index + 1}`,
        ).toEqual(step.expect);
      }
    } finally {
      stop();
      polls.at(-1)?.next.resolve({ cursor: 12, events: [], overflow: false });
    }
  });
});
