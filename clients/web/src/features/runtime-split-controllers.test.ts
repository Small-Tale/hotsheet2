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

const streams = vi.hoisted(() => ({ started: [] as string[], stopped: [] as string[] }));
vi.mock('../project-change-poll', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../project-change-poll')>()),
  startProjectChangeStream: (options: { client: { origin?: string } }) => {
    const id = (options.client as unknown as { origin: string }).origin;
    streams.started.push(id);
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
        applyTerminalRenamed: vi.fn(),
        refreshTerminalDashboard: noop,
        refreshProviderOutbox: noop,
        refreshCommands: noop,
        refreshCustomViews: noop,
        refreshRepositoryStatus: noop,
        loadConfidenceReport: noop,
        setBackgroundProjectRefresh: vi.fn(),
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

  it('workspace search: debounces repeated edits into one search and clears when the query empties', async () => {
    const searchQuery = signal(''),
      searchTokens = signal<InlineSearchToken[]>([]),
      searchMatchKeys = signal<Set<string> | undefined>(undefined),
      tickets = signal<TicketRow[]>([]),
      refreshProject = vi.fn(async () => undefined),
      fetchMock = vi.fn<typeof fetch>(async () => Response.json({ items: [], next_cursor: null })),
      persist = vi.fn();
    vi.stubGlobal('fetch', fetchMock);
    const owner = createWorkspaceSearchController({
      searchQuery,
      searchTokens,
      searchHelpOpen: signal(false),
      searchMatchKeys,
      sidebarSearchCounts: signal<SidebarSearchCounts | undefined>(undefined),
      selectedView: signal<TicketView>('all'),
      error: signal(''),
      ticketPageQuery: signal<CheckoutTicketQuery>({}),
      ticketNextCursor: signal(undefined),
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
    replaceTicketSearch(owner.workspaceSearchModel, '');
    await owner.refreshTicketSearch();
    expect(searchMatchKeys.value).toBeUndefined();
    expect(refreshProject).toHaveBeenCalledWith({ showLoading: false });
  });
});
