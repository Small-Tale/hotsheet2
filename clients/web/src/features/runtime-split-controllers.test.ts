import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { CheckoutTicketQuery, TicketRow } from '../api';
import type { ProjectCloseDialogState } from '../components/project-close-dialog';
import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import type { InlineSearchToken } from '../inline-search';
import type { Project } from '../interactions/types';
import { createLiveStateStore } from '../live-state';
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
  onResync: new Map<string, (reason: 'initial' | 'reconnect' | 'overflow') => Promise<void>>(),
  refresh: new Map<string, () => Promise<void>>(),
  onError: new Map<string, (reason: unknown) => void>(),
}));
vi.mock('../project-change-poll', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../project-change-poll')>()),
  startProjectChangeStream: (options: {
    client: { origin?: string };
    onEvents: (response: unknown) => Promise<void>;
    onResync: (reason: 'initial' | 'reconnect' | 'overflow') => Promise<void>;
    refresh: () => Promise<void>;
    onError?: (reason: unknown) => void;
  }) => {
    const id = (options.client as unknown as { origin: string }).origin;
    streams.started.push(id);
    streams.onEvents.set(id, options.onEvents);
    streams.onResync.set(id, options.onResync);
    streams.refresh.set(id, options.refresh);
    if (options.onError) streams.onError.set(id, options.onError);
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
  streams.onResync.clear();
  streams.refresh.clear();
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

  it('change streams: burst refreshes are bounded per project while other events keep flowing', async () => {
    const projects = signal([project('a'), project('b')]);
    const noop = vi.fn(async () => undefined);
    const refreshed: string[] = [];
    const refreshTab = vi.fn(async (target: Project) => {
      refreshed.push(target.id);
    });
    const owner = createProjectChangeStreamsController({
      projects,
      project: () => projects.value[0],
      shellMode: signal('project'),
      statsProjectId: signal(undefined),
      localTicketMutationBarrier: createRefreshBarrier(),
      localTicketChangeAcknowledgements: new LocalTicketChangeAcknowledgements(),
      projectTabRefresh: { request: refreshTab },
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
    for (let index = 0; index < 20; index += 1) {
      await streams.refresh.get('/api/a')!();
      await streams.refresh.get('/api/b')!();
    }
    expect(refreshTab).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(180);
    expect(refreshed.sort()).toEqual(['a', 'b']);
    projects.value = [project('a')];
    owner.syncProjectChangeStreams();
    await streams.refresh.get('/api/b')!();
    owner.dispose();
    await vi.advanceTimersByTimeAsync(1000);
    expect(refreshTab).toHaveBeenCalledTimes(2);
  });

  it('change streams: permission events and resyncs refresh only their own project', async () => {
    const projects = signal([project('a'), project('b')]),
      noop = vi.fn(async () => undefined),
      refreshPermissions = vi.fn(async () => undefined),
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
        refreshPermissions,
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
    await streams.onResync.get('/api/a')!('initial');
    await streams.onResync.get('/api/b')!('reconnect');
    await streams.onEvents.get('/api/b')!({
      events: [
        { kind: 'permission_asked', id: '1' },
        { kind: 'permission_resolved', id: '2' },
      ],
    });
    await streams.onEvents.get('/api/a')!({ events: [{ kind: 'permission_asked', id: '3' }] });
    expect(refreshPermissions.mock.calls).toEqual([['a'], ['b'], ['b'], ['a']]);
    owner.dispose();
  });

  it('change streams: live_state events apply, each resync reads live state once, idle time reads nothing (HS2-JTTQMY)', async () => {
    const snapshot = (revision: number, state: string) => ({
      store: 's1',
      root: '/work/a.hs2',
      name: 'a',
      checkouts: ['a'],
      epoch_ms: 1,
      revision,
      state,
      activity: {
        word: state === 'waiting_on_you' ? 'active' : state,
        active_since_ms: 1,
        stopped_at_ms: null,
        last_progress_ms: 1,
        idle_deadline_ms: null,
        stalled: [],
        halted: [],
        causes: [],
      },
      claims: [],
      workers: { live: 0, expected: 0 },
      waiting: { count: 0, tickets: [] },
      runway: { ready: 0, estimate_ms: null },
      rates: {
        tokens: { window_minutes: 5, minutes: [], peak_per_minute: 0 },
        tickets_updated: { window_minutes: 60, at_ms: [], peak_per_hour: 0 },
      },
    });
    const reads: string[] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        reads.push(url);
        return new Response(JSON.stringify({ epoch_ms: 1, stores: [snapshot(2, 'active')] }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        });
      }),
    );
    const projects = signal([project('a')]),
      noop = vi.fn(async () => undefined),
      liveStates = createLiveStateStore(),
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
        liveStates,
      });
    owner.syncProjectChangeStreams();
    await streams.onResync.get('/api/a')!('initial');
    expect(reads.filter((url) => url.endsWith('/live-state'))).toEqual(['/api/a/live-state']);
    expect(liveStates.forProject('a')[0]?.revision).toBe(2);
    await streams.onEvents.get('/api/a')!({
      cursor: 9,
      overflow: false,
      events: [{ kind: 'live_state', store: 's1', id: 's1', slug: '', live: snapshot(3, 'stalled') }],
    });
    expect(liveStates.states.value.get('s1')?.state).toBe('stalled');
    // An hour without events: no live-state read at all.
    await vi.advanceTimersByTimeAsync(60 * 60_000);
    expect(reads.filter((url) => url.endsWith('/live-state'))).toHaveLength(1);
    // A reconnect reads once more; its older snapshot does not undo the pushed one.
    await streams.onResync.get('/api/a')!('reconnect');
    expect(reads.filter((url) => url.endsWith('/live-state'))).toHaveLength(2);
    expect(liveStates.states.value.get('s1')).toMatchObject({ revision: 3, state: 'stalled' });
    owner.dispose();
  });

  it('change streams: host_health events and resyncs feed the host health resource, idle time reads nothing (HS2-NS0AYR)', async () => {
    const projects = signal([project('a'), project('b')]),
      noop = vi.fn(async () => undefined),
      hostHealth = { applyEvents: vi.fn(() => undefined), resync: vi.fn(async () => undefined) },
      owner = createProjectChangeStreamsController({
        projects,
        project: () => projects.value[0],
        shellMode: signal('stats'),
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
        hostHealth,
      });
    owner.syncProjectChangeStreams();
    await streams.onResync.get('/api/a')!('initial');
    await streams.onResync.get('/api/b')!('reconnect');
    expect(hostHealth.resync).toHaveBeenCalledTimes(2);
    const events = [
      { cursor: 4, store: '', kind: 'host_health', id: 'disk', slug: 'warning', message: 'low_free_space' },
    ];
    await streams.onEvents.get('/api/a')!({ cursor: 4, overflow: false, events });
    expect(hostHealth.applyEvents).toHaveBeenCalledWith(events);
    await vi.advanceTimersByTimeAsync(60 * 60_000);
    expect(hostHealth.resync).toHaveBeenCalledTimes(2);
    expect(hostHealth.applyEvents).toHaveBeenCalledTimes(1);
    owner.dispose();
  });

  it('change streams: report each stream health for standby freshness, not handler failures (HS2-EQVZTZ)', async () => {
    const projects = signal([project('a'), project('b')]),
      noop = vi.fn(async () => undefined),
      health: [string, boolean | undefined][] = [],
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
        // A resync whose own reads fail leaves the connection healthy.
        refreshPermissions: vi.fn(() => Promise.reject(new Error('permissions unavailable'))),
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
        onStreamHealth: (id, live) => {
          health.push([id, live]);
        },
      });
    owner.syncProjectChangeStreams();
    await expect(streams.onResync.get('/api/a')!('initial')).resolves.toBeUndefined();
    streams.onError.get('/api/a')!(new Error('Project change WebSocket closed.'));
    await streams.onEvents.get('/api/a')!({ events: [{ kind: 'updated', id: 'x' }] });
    streams.onError.get('/api/b')!(new Error('offline'));
    projects.value = [project('a')];
    owner.syncProjectChangeStreams();
    expect(health).toEqual([
      ['a', true],
      ['a', false],
      ['a', true],
      ['b', false],
      ['b', undefined],
    ]);
    owner.dispose();
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

  it('change streams: a stamped turn event samples that project server clock and times the event by it (HS2-GA5ASE)', async () => {
    const projects = signal([project('a'), project('b')]),
      noop = vi.fn(async () => undefined),
      observe = vi.fn(),
      updateConversation = vi.fn(),
      turn = {
        connection_id: 'chat-1',
        turn_id: 't1',
        at_ms: 1_800_000_180_000,
        event: { type: 'output', content: 'hi', truncated: false },
      },
      owner = createProjectChangeStreamsController({
        projects,
        project: () => projects.value[0],
        shellMode: signal('project'),
        statsProjectId: signal(undefined),
        localTicketMutationBarrier: createRefreshBarrier(),
        localTicketChangeAcknowledgements: new LocalTicketChangeAcknowledgements(),
        projectTabRefresh: { request: noop },
        turnStreamEvents: (response) => (response.events as { turn?: unknown }[]).map((event) => event.turn),
        conversationPersistence: { flush: vi.fn() } as never,
        updateConversation,
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
        serverClocks: { observe, now: () => 1_800_000_200_000, offset: () => 180_000 },
      });
    owner.syncProjectChangeStreams();
    await streams.onEvents.get('/api/b')!({ events: [{ kind: 'turn_event', id: '', turn }] });
    expect(observe).toHaveBeenCalledWith('b', 1_800_000_180_000);
    // An older server's unstamped event is timed at the project's server clock, not the device's.
    const unstamped = { connection_id: turn.connection_id, turn_id: turn.turn_id, event: turn.event };
    await streams.onEvents.get('/api/b')!({ events: [{ kind: 'turn_event', id: '', turn: unstamped }] });
    type Update = (state: { messages: never[] }) => { turnTiming?: { lastEventAt?: number } };
    const [stamped, fallback] = updateConversation.mock.calls.map((call): Update => call[1]);
    expect(stamped({ messages: [] }).turnTiming?.lastEventAt).toBe(1_800_000_180_000);
    expect(fallback({ messages: [] }).turnTiming?.lastEventAt).toBe(1_800_000_200_000);
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
  it('workspace review search pushes a lone term down and walks Boolean result pages', async () => {
    const searchQuery = signal(''),
      searchTokens = signal<InlineSearchToken[]>([]),
      searchMatchKeys = signal<Set<string> | undefined>(undefined),
      tickets = signal<TicketRow[]>([]),
      calls: string[] = [],
      row = (slug: string, needsReview: boolean): TicketRow => ({
        connection_id: 'git',
        native_id: slug,
        qualified_id: `git:${slug}`,
        id: slug,
        slug,
        title: slug,
        status: 'started',
        up_next: false,
        feedback_needed: needsReview,
        needs_review: needsReview,
        tags: [],
        blocked_by: [],
        claim_count: 0,
      });
    vi.stubGlobal(
      'fetch',
      vi.fn<typeof fetch>(async (url) => {
        const value = typeof url === 'string' ? url : url instanceof URL ? url.href : url.url;
        calls.push(value);
        const params = new URL(value, 'http://local').searchParams;
        const boolean = !params.has('needs_review');
        return Response.json({
          items: boolean
            ? params.has('cursor')
              ? [row('HS2-LATE', true)]
              : [row('HS2-FIRST', false)]
            : [row('HS2-LATE', true)],
          next_cursor: boolean && !params.has('cursor') ? 'second' : null,
          counts: { total: 2, queued: 2, backlog: 0, archive: 0, open: 2, up_next: 0, active: 0, started: 2 },
        });
      }),
    );
    const owner = createWorkspaceSearchController({
      searchQuery,
      searchTokens,
      searchHelpOpen: signal(false),
      searchMatchKeys,
      sidebarSearchCounts: signal<SidebarSearchCounts | undefined>(undefined),
      selectedView: signal<TicketView>('all'),
      searchAllTickets: signal(false),
      error: signal(''),
      ticketPageQuery: signal<CheckoutTicketQuery>({}),
      ticketNextCursor: signal<string | undefined>(undefined),
      tickets,
      ticketRowsByProject: signal({}),
      project: () => project('a'),
      customViewFor: () => undefined,
      customViewsFor: () => [],
      activeWorkspaceSort: () => ({ sort: 'priority', sortDirection: 'descending' }),
      ticketSearchKey: (ticket) => `${ticket.connection_id}:${ticket.native_id}`,
      mergeTicketLinkRows: (_existing, incoming) => [...incoming],
      refreshProject: vi.fn(async () => undefined),
      resetBoardColumnPages: vi.fn(),
      resetProgressiveTicketRendering: vi.fn(),
      scheduleProjectSessionPersistence: vi.fn(),
    });
    replaceTicketSearch(owner.workspaceSearchModel, 'is:needs-review');
    await owner.refreshTicketSearch();
    expect(calls.some((url) => url.includes('needs_review=true'))).toBe(true);
    expect(searchMatchKeys.value).toEqual(new Set(['git:HS2-LATE']));
    calls.length = 0;
    replaceTicketSearch(owner.workspaceSearchModel, 'is:needs-review OR HS2-FIRST');
    await owner.refreshTicketSearch();
    expect(calls.some((url) => url.includes('cursor=second'))).toBe(true);
    expect(calls.every((url) => !url.includes('needs_review=true'))).toBe(true);
    expect(searchMatchKeys.value).toEqual(new Set(['git:HS2-FIRST', 'git:HS2-LATE']));
  });
  it('workspace Boolean search uses complete qualified membership and refuses unavailable NOT (HS2-DE9CMP)', async () => {
    const first: TicketRow = {
        connection_id: 'git',
        native_id: 'one',
        qualified_id: 'git:one',
        id: 'one',
        slug: 'HS2-ONE',
        title: 'First',
        status: 'started',
        up_next: false,
        feedback_needed: false,
        tags: ['docs'],
        blocked_by: [],
        claim_count: 0,
      },
      second: TicketRow = {
        ...first,
        connection_id: 'other',
        native_id: 'one',
        qualified_id: 'other:one',
        id: 'two',
        slug: 'HS2-TWO',
        title: 'Second',
        tags: [],
      },
      searchQuery = signal(''),
      searchTokens = signal<InlineSearchToken[]>([]),
      searchMatchKeys = signal<Set<string> | undefined>(undefined),
      error = signal(''),
      calls: URL[] = [];
    let unavailable = false;
    vi.stubGlobal(
      'fetch',
      vi.fn<typeof fetch>(async (url) => {
        const parsed = new URL(typeof url === 'string' ? url : url instanceof URL ? url.href : url.url, 'http://local'),
          params = parsed.searchParams,
          membership = params.get('has_attachment') === 'true';
        calls.push(parsed);
        return Response.json({
          items: membership ? (params.has('cursor') ? [second] : []) : [first, second],
          next_cursor: membership && !params.has('cursor') ? 'membership-next' : null,
          counts:
            params.get('counts') === 'false'
              ? null
              : { total: 2, queued: 2, backlog: 0, archive: 0, open: 2, up_next: 0, active: 0, started: 2 },
          source_errors: membership && unavailable ? ['provider does not support attachment search'] : [],
        });
      }),
    );
    const owner = createWorkspaceSearchController({
      searchQuery,
      searchTokens,
      searchHelpOpen: signal(false),
      searchMatchKeys,
      sidebarSearchCounts: signal<SidebarSearchCounts | undefined>(undefined),
      selectedView: signal<TicketView>('all'),
      searchAllTickets: signal(false),
      error,
      ticketPageQuery: signal<CheckoutTicketQuery>({}),
      ticketNextCursor: signal<string | undefined>(undefined),
      tickets: signal<TicketRow[]>([]),
      ticketRowsByProject: signal({}),
      project: () => project('a'),
      customViewFor: () => undefined,
      customViewsFor: () => [],
      activeWorkspaceSort: () => ({ sort: 'priority', sortDirection: 'descending' }),
      ticketSearchKey: (ticket) => `${ticket.connection_id}:${ticket.native_id}`,
      mergeTicketLinkRows: (_existing, incoming) => [...incoming],
      refreshProject: vi.fn(async () => undefined),
      resetBoardColumnPages: vi.fn(),
      resetProgressiveTicketRendering: vi.fn(),
      scheduleProjectSessionPersistence: vi.fn(),
    });
    replaceTicketSearch(owner.workspaceSearchModel, 'tag:docs OR has:attachment');
    await owner.refreshTicketSearch();
    expect(searchMatchKeys.value).toEqual(new Set(['git:one', 'other:one']));
    calls.length = 0;
    replaceTicketSearch(owner.workspaceSearchModel, 'NOT has:attachment');
    await owner.refreshTicketSearch();
    expect(searchMatchKeys.value).toEqual(new Set(['git:one']));
    expect(
      calls.some(
        (call) => call.searchParams.get('has_attachment') === 'true' && call.searchParams.get('counts') === 'false',
      ),
    ).toBe(true);
    expect(calls.some((call) => call.searchParams.get('cursor') === 'membership-next')).toBe(true);
    expect(
      calls.some((call) => call.searchParams.get('fields') === 'slug,connection_id,native_id,qualified_id,id'),
    ).toBe(true);
    unavailable = true;
    await owner.refreshTicketSearch();
    expect(searchMatchKeys.value).toEqual(new Set());
    expect(error.value).toContain('Structured search unavailable');
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
