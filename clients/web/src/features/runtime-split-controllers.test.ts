import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { CheckoutTicketQuery, TicketRow } from '../api';
import type { ProjectCloseDialogState } from '../components/project-close-dialog';
import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import type { InlineSearchToken } from '../inline-search';
import type { Project } from '../interactions/types';
import { LocalTicketChangeAcknowledgements } from '../local-ticket-changes';
import { createRefreshBarrier } from '../refresh-barrier';
import { replaceTicketSearch } from '../ticket-search-model';
import type { TicketView } from '../ticket-views';
import { createProjectChangeStreamsController } from './project-change-streams';
import { createProjectCloseController } from './project-close';
import { createWorkspaceSearchController, type SidebarSearchCounts } from './workspace-search';

const streams = vi.hoisted(() => ({
  started: [] as string[],
  stopped: [] as string[],
  onEvents: new Map<string, (response: unknown) => Promise<void>>(),
}));
vi.mock('../project-change-poll', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../project-change-poll')>()),
  startProjectChangeStream: (options: {
    client: { origin?: string };
    onEvents: (response: unknown) => Promise<void>;
  }) => {
    const id = (options.client as unknown as { origin: string }).origin;
    streams.started.push(id);
    streams.onEvents.set(id, options.onEvents);
    return () => streams.stopped.push(id);
  },
}));

const project = (id: string): Project => ({
  id,
  name: id,
  root: `/work/${id}`,
  apiPath: `/api/${id}`,
  stores: [],
  compatibility: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
});

beforeEach(() => {
  vi.useFakeTimers();
  streams.started.length = 0;
  streams.stopped.length = 0;
  streams.onEvents.clear();
  vi.stubGlobal('window', { setTimeout, clearTimeout });
  vi.stubGlobal('requestAnimationFrame', (callback: () => void) => setTimeout(callback, 0));
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('runtime-split feature owners (HS2-3JGWTV)', () => {
  it('project close: walks the confirmation queue, skips vanished projects, and cancels cleanly', async () => {
    const projects = signal([project('a'), project('b'), project('c')]),
      projectCloseDialog = signal<ProjectCloseDialogState | undefined>(undefined),
      closeProjectIds = vi.fn((ids: readonly string[]) => {
        projects.value = projects.value.filter((item) => !ids.includes(item.id));
      }),
      owner = createProjectCloseController({
        projects,
        projectCloseDialog,
        terminalGroups: signal<TerminalDashboardGroup[]>([
          {
            projectId: 'a',
            projectName: 'a',
            sessions: [
              { id: 't1', alive: true, title: 'shell' },
              { id: 't2', alive: false },
            ],
          } as unknown as TerminalDashboardGroup,
        ]),
        terminalDrawerChatsByProject: signal({}),
        driveConnectionsByProject: signal({}),
        conversationStates: signal({}),
        aiToolLabel: (tool) => tool,
        closeProjectIds,
      });
    owner.requestProjectClose(['a', 'a', 'missing', 'c']);
    expect(projectCloseDialog.value).toMatchObject({ projectId: 'a', resources: [{ kind: 'terminal', id: 't1' }] });
    owner.confirmProjectClose();
    expect(closeProjectIds).toHaveBeenCalledWith(['a']);
    expect(projectCloseDialog.value).toBeUndefined();
    await Promise.resolve();
    expect(projectCloseDialog.value).toMatchObject({ projectId: 'c', resources: [] });
    owner.cancelProjectClose();
    expect(projectCloseDialog.value).toBeUndefined();
    // A cancelled queue does not resurface on the next microtask.
    await Promise.resolve();
    expect(projectCloseDialog.value).toBeUndefined();
    expect(closeProjectIds).toHaveBeenCalledTimes(1);
  });

  it('change streams: starts one stream per open project and stops closed ones (empty-then-refill)', () => {
    const projects = signal([project('a'), project('b')]),
      noop = vi.fn(async () => undefined),
      owner = createProjectChangeStreamsController({
        projects,
        project: () => projects.value[0],
        shellMode: signal('project'),
        statsProjectId: signal(undefined),
        localTicketMutationBarrier: createRefreshBarrier(),
        localTicketChangeAcknowledgements: new LocalTicketChangeAcknowledgements(),
        projectTabRefresh: { request: noop },
        turnStreamEvents: () => [],
        conversationPersistence: { flush: vi.fn() } as never,
        updateConversation: vi.fn(),
        conversationForActivity: vi.fn(),
        refreshDriveConnections: noop,
        refreshPermissions: noop,
        serverResolvedPermission: vi.fn(),
        permissionRulesChanged: vi.fn(),
        applyTerminalRenamed: vi.fn(),
        refreshTerminalDashboard: noop,
        refreshProviderOutbox: noop,
        refreshCommands: noop,
        refreshCustomViews: noop,
        refreshRepositoryStatus: noop,
        loadConfidenceReport: noop,
        loadRepositorySize: noop,
      });
    owner.syncProjectChangeStreams();
    owner.syncProjectChangeStreams();
    expect(streams.started).toEqual(['/api/a', '/api/b']);
    projects.value = [];
    owner.syncProjectChangeStreams();
    expect(streams.stopped).toEqual(['/api/a', '/api/b']);
    projects.value = [project('b')];
    owner.syncProjectChangeStreams();
    expect(streams.started).toEqual(['/api/a', '/api/b', '/api/b']);
  });

  it('change streams: a repository_size event reloads only that project size report (HS2-KKFEY5)', async () => {
    const projects = signal([project('a'), project('b')]),
      noop = vi.fn(async () => undefined),
      loadRepositorySize = vi.fn(async () => undefined),
      owner = createProjectChangeStreamsController({
        projects,
        project: () => projects.value[0],
        shellMode: signal('project'),
        statsProjectId: signal(undefined),
        localTicketMutationBarrier: createRefreshBarrier(),
        localTicketChangeAcknowledgements: new LocalTicketChangeAcknowledgements(),
        projectTabRefresh: { request: noop },
        turnStreamEvents: () => [],
        conversationPersistence: { flush: vi.fn() } as never,
        updateConversation: vi.fn(),
        conversationForActivity: vi.fn(),
        refreshDriveConnections: noop,
        refreshPermissions: noop,
        serverResolvedPermission: vi.fn(),
        applyTerminalRenamed: vi.fn(),
        refreshTerminalDashboard: noop,
        refreshProviderOutbox: noop,
        refreshCommands: noop,
        refreshCustomViews: noop,
        refreshRepositoryStatus: noop,
        loadConfidenceReport: noop,
        loadRepositorySize,
        permissionRulesChanged: vi.fn(),
      });
    owner.syncProjectChangeStreams();
    await streams.onEvents.get('/api/a')!({
      events: [{ kind: 'repository_size', id: 'a', slug: 'warning', message: 'over the soft limit' }],
    });
    expect(loadRepositorySize).toHaveBeenCalledTimes(1);
    expect(loadRepositorySize).toHaveBeenCalledWith(projects.value[0]);
    // Another checkout's crossing, or an unrelated event, reloads nothing here.
    await streams.onEvents.get('/api/a')!({ events: [{ kind: 'repository_size', id: 'b' }] });
    await streams.onEvents.get('/api/a')!({ events: [{ kind: 'repository_changed', id: 'a' }] });
    expect(loadRepositorySize).toHaveBeenCalledTimes(1);
    owner.dispose();
  });

  it('change streams: dispose stops every stream and pending repository refresh; restart builds a fresh owner (HS2-A9E7QB)', async () => {
    const projects = signal([project('a'), project('b')]),
      noop = vi.fn(async () => undefined),
      refreshRepositoryStatus = vi.fn(async () => undefined),
      permissionRulesChanged = vi.fn(),
      build = () =>
        createProjectChangeStreamsController({
          projects,
          project: () => projects.value[0],
          shellMode: signal('project'),
          statsProjectId: signal(undefined),
          localTicketMutationBarrier: createRefreshBarrier(),
          localTicketChangeAcknowledgements: new LocalTicketChangeAcknowledgements(),
          projectTabRefresh: { request: noop },
          turnStreamEvents: () => [],
          conversationPersistence: { flush: vi.fn() } as never,
          updateConversation: vi.fn(),
          conversationForActivity: vi.fn(),
          refreshDriveConnections: noop,
          refreshPermissions: noop,
          serverResolvedPermission: vi.fn(),
          permissionRulesChanged,
          applyTerminalRenamed: vi.fn(),
          refreshTerminalDashboard: noop,
          refreshProviderOutbox: noop,
          refreshCommands: noop,
          refreshCustomViews: noop,
          refreshRepositoryStatus,
          loadConfidenceReport: noop,
          loadRepositorySize: noop,
        });
    const owner = build();
    owner.syncProjectChangeStreams();
    // A repository change schedules the debounced refresh; disposing before it fires cancels it.
    await streams.onEvents.get('/api/a')!({ events: [{ kind: 'repository_changed', id: 'a' }] });
    owner.dispose();
    expect(streams.stopped).toEqual(['/api/a', '/api/b']);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(refreshRepositoryStatus).not.toHaveBeenCalled();
    // Repeated dispose, a late sync (startup restore), and a late stream event are all inert.
    owner.dispose();
    owner.syncProjectChangeStreams();
    await streams.onEvents.get('/api/a')!({ events: [{ kind: 'repository_changed', id: 'a' }] });
    await vi.advanceTimersByTimeAsync(1_000);
    expect(streams.started).toEqual(['/api/a', '/api/b']);
    expect(streams.stopped).toEqual(['/api/a', '/api/b']);
    expect(refreshRepositoryStatus).not.toHaveBeenCalled();
    // Restart: a new runtime's owner opens fresh streams and schedules refreshes again.
    const restarted = build();
    restarted.syncProjectChangeStreams();
    expect(streams.started).toEqual(['/api/a', '/api/b', '/api/a', '/api/b']);
    await streams.onEvents.get('/api/a')!({ events: [{ kind: 'repository_changed', id: 'a' }] });
    await vi.advanceTimersByTimeAsync(200);
    expect(refreshRepositoryStatus).toHaveBeenCalledTimes(1);
    // A remembered answer added or revoked elsewhere marks only that project's rules stale (HS2-KYK1S7).
    await streams.onEvents.get('/api/b')!({ events: [{ kind: 'permission_rules_changed', id: '' }] });
    expect(permissionRulesChanged).toHaveBeenCalledExactlyOnceWith('b');
    restarted.dispose();
  });
  it('workspace search: debounces repeated edits into one search and clears when the query empties', async () => {
    const searchQuery = signal(''),
      searchTokens = signal<InlineSearchToken[]>([]),
      searchMatchKeys = signal<Set<string> | undefined>(undefined),
      tickets = signal<TicketRow[]>([]),
      refreshProject = vi.fn(async () => undefined),
      fetchMock = vi.fn<typeof fetch>(async () => Response.json({ items: [], next_cursor: null })),
      ticketPageQuery = signal<CheckoutTicketQuery>({}),
      ticketNextCursor = signal<string | undefined>(undefined),
      persist = vi.fn();
    vi.stubGlobal('fetch', fetchMock);
    const owner = createWorkspaceSearchController({
      searchQuery,
      searchTokens,
      searchHelpOpen: signal(false),
      searchMatchKeys,
      sidebarSearchCounts: signal<SidebarSearchCounts | undefined>(undefined),
      selectedView: signal<TicketView>('all'),
      searchAllTickets: signal(false),
      error: signal(''),
      ticketPageQuery,
      ticketNextCursor,
      tickets,
      ticketRowsByProject: signal({}),
      project: () => project('a'),
      customViewFor: () => undefined,
      customViewsFor: () => [],
      activeWorkspaceSort: () => ({ sort: 'priority', sortDirection: 'descending' }),
      ticketSearchKey: (ticket) => `${ticket.connection_id}:${ticket.native_id}`,
      mergeTicketLinkRows: (_existing, incoming) => [...incoming],
      refreshProject,
      resetBoardColumnPages: vi.fn(),
      resetProgressiveTicketRendering: vi.fn(),
      scheduleProjectSessionPersistence: persist,
    });
    expect(owner.workspaceSearchActive()).toBe(false);
    // Edits arrive through Kerf's search model; its effect projects them and schedules the search.
    replaceTicketSearch(owner.workspaceSearchModel, 'a');
    replaceTicketSearch(owner.workspaceSearchModel, 'ab');
    expect(searchQuery.value).toBe('ab');
    expect(owner.searchBarActive()).toBe(true);
    await vi.advanceTimersByTimeAsync(200);
    const searches = fetchMock.mock.calls.filter(([url]) => typeof url === 'string' && url.includes('/tickets'));
    expect(searches.length).toBeGreaterThanOrEqual(1);
    expect(searchMatchKeys.value).toEqual(new Set());
    expect(persist).toHaveBeenCalledTimes(2);
    // A search page left a continuation bound to the search query.
    ticketNextCursor.value = 'search-cursor';
    ticketPageQuery.value = { text: 'ab' };
    replaceTicketSearch(owner.workspaceSearchModel, '');
    await owner.refreshTicketSearch();
    expect(searchMatchKeys.value).toBeUndefined();
    // The search cursor leaves with its query, so Load more cannot send it with the base query (HS2-38MPMC).
    expect(ticketNextCursor.value).toBeUndefined();
    expect(ticketPageQuery.value).toEqual({});
    expect(refreshProject).toHaveBeenCalledWith({ showLoading: false });
  });
  it('workspace search: All Tickets scope toggles, re-scopes the request, and ends with the search (HS2-4J6764)', async () => {
    const searchQuery = signal(''),
      searchTokens = signal<InlineSearchToken[]>([]),
      searchMatchKeys = signal<Set<string> | undefined>(undefined),
      sidebarSearchCounts = signal<SidebarSearchCounts | undefined>(undefined),
      selectedView = signal<TicketView>('all'),
      searchAllTickets = signal(false),
      fetchMock = vi.fn<typeof fetch>(async () =>
        Response.json({
          items: [],
          counts: { total: 0, queued: 0, backlog: 0, archive: 0, open: 0, up_next: 0, active: 0, started: 0 },
        }),
      );
    vi.stubGlobal('fetch', fetchMock);
    const owner = createWorkspaceSearchController({
      searchQuery,
      searchTokens,
      searchHelpOpen: signal(false),
      searchMatchKeys,
      sidebarSearchCounts,
      selectedView,
      searchAllTickets,
      error: signal(''),
      ticketPageQuery: signal<CheckoutTicketQuery>({}),
      ticketNextCursor: signal<string | undefined>(undefined),
      tickets: signal<TicketRow[]>([]),
      ticketRowsByProject: signal({}),
      project: () => project('a'),
      customViewFor: (view) =>
        view === 'custom:mine' ? { id: 'mine', name: 'Mine', query: 'status:started' } : undefined,
      customViewsFor: () => [{ id: 'mine', name: 'Mine', query: 'status:started' }],
      activeWorkspaceSort: () => ({ sort: 'priority', sortDirection: 'descending' }),
      ticketSearchKey: (ticket) => `${ticket.connection_id}:${ticket.native_id}`,
      mergeTicketLinkRows: (_existing, incoming) => [...incoming],
      refreshProject: vi.fn(async () => undefined),
      resetBoardColumnPages: vi.fn(),
      resetProgressiveTicketRendering: vi.fn(),
      scheduleProjectSessionPersistence: vi.fn(),
    });
    const scopes = () =>
      fetchMock.mock.calls
        .map(([url]) => new URL(url instanceof Request ? url.url : url, 'http://x'))
        .filter((url) => url.pathname.endsWith('/tickets') && url.searchParams.get('text') === 'badge')
        .map((url) => url.searchParams.get('status') ?? url.searchParams.get('collection'));
    // Without a search there is nothing to widen: the toggle is unavailable and inert.
    expect(owner.allTicketsSearchAvailable()).toBe(false);
    owner.setAllTicketsSearch(true);
    expect(searchAllTickets.value).toBe(false);
    replaceTicketSearch(owner.workspaceSearchModel, 'badge');
    await vi.advanceTimersByTimeAsync(200);
    expect(owner.searchScope()).toBe('all');
    expect(scopes()[0]).toBe('queue');
    await vi.waitFor(() => {
      expect(sidebarSearchCounts.value?.pending).toEqual([]);
    });
    expect(sidebarSearchCounts.value?.values).toEqual({ all: 0, backlog: 0, archive: 0, 'custom:mine': 0 });
    // Entering All Tickets searches every collection except Trash and counts each sidebar view itself.
    fetchMock.mockClear();
    owner.setAllTicketsSearch(true);
    expect(owner.searchScope()).toBe('all-tickets');
    await vi.advanceTimersByTimeAsync(0);
    expect(scopes()[0]).toBe('all');
    expect(scopes()).not.toContain('trash');
    expect(sidebarSearchCounts.value?.pending).toEqual([]);
    // Toggling again returns to the view the search started from, reusing its settled counts.
    fetchMock.mockClear();
    owner.setAllTicketsSearch(false);
    await vi.advanceTimersByTimeAsync(0);
    expect(owner.searchScope()).toBe('all');
    expect(scopes()).toEqual(['queue']);
    // A shared view's own query drops out while All Tickets covers everything.
    selectedView.value = 'custom:mine';
    expect(owner.selectedViewSearch().text).toContain('status:started');
    owner.setAllTicketsSearch(true);
    expect(owner.searchScope()).toBe('all-tickets');
    expect(owner.selectedViewSearch().text).not.toContain('status:started');
    // Clearing the search ends the mode, so the next search starts in its own view again.
    replaceTicketSearch(owner.workspaceSearchModel, '');
    await vi.advanceTimersByTimeAsync(200);
    expect(searchAllTickets.value).toBe(false);
    replaceTicketSearch(owner.workspaceSearchModel, 'badge');
    expect(owner.searchScope()).toBe('custom:mine');
    // Trash never offers All Tickets, even with the mode left on.
    selectedView.value = 'trash';
    searchAllTickets.value = true;
    expect(owner.allTicketsSearchAvailable()).toBe(false);
    expect(owner.searchScope()).toBe('trash');
    owner.setAllTicketsSearch(false);
    expect(searchAllTickets.value).toBe(true);
    // Choosing the scope already in effect does nothing.
    selectedView.value = 'all';
    fetchMock.mockClear();
    owner.setAllTicketsSearch(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(scopes()).toEqual([]);
  });
});
