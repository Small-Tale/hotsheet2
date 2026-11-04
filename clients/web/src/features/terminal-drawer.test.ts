import { effect, signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { ToolConnection } from '../api';
import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import { drawerSelectionView, type PendingDrawerTab } from '../drawer-tab-order';
import type { Project } from '../interactions/types';
import type { DrawerAIChat } from '../project-drive';
import { createTerminalDrawerController } from './terminal-drawer';

const project: Project = {
  id: 'a',
  name: 'a',
  root: '/work/a',
  apiPath: '/api/a',
  stores: [],
  compatibility: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
};
const fetchMock = vi.fn<typeof fetch>();
const stored = new Map<string, string>();
const json = (body: unknown) => Promise.resolve(new Response(JSON.stringify(body), { status: 200 }));

beforeEach(() => {
  vi.useFakeTimers();
  fetchMock.mockReset();
  stored.clear();
  vi.stubGlobal('fetch', fetchMock);
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
  });
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) =>
    setTimeout(() => {
      callback(0);
    }, 0),
  );
  vi.stubGlobal('window', { innerWidth: 1280, setTimeout, clearTimeout });
  vi.stubGlobal('document', { activeElement: null, body: null, querySelectorAll: () => [] });
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function owner(tabIds: () => string[] = () => ['t1', 't2', 'ai-chat:c']) {
  const bindings = {
      projects: signal([project]),
      selectedProjectId: signal('a'),
      terminalGroups: signal<TerminalDashboardGroup[]>([]),
      terminalDrawerVisible: signal(false),
      terminalDrawerMounted: signal(false),
      terminalDrawerTransitioning: signal(false),
      terminalDrawerSelected: signal('grid'),
      terminalDrawerChatsByProject: signal<Record<string, DrawerAIChat[]>>({}),
      terminalDrawerPendingByProject: signal<Record<string, PendingDrawerTab[]>>({}),
      terminalDashboardLoading: signal(false),
      terminalDashboardMessage: signal(''),
      terminalVisibility: signal({ activeGroupId: 'all', groups: [] } as never),
      terminalContextMenu: signal<{ key: string; x: number; y: number } | undefined>(undefined),
      appTabContextMenu: signal<never>(undefined),
      error: signal(''),
      conversationConnectionId: signal<string | undefined>(undefined),
      driveConnectionsByProject: signal<Record<string, ToolConnection[]>>({}),
      drivePendingByProject: signal<Record<string, boolean>>({}),
    },
    calls = {
      refreshTerminalDashboard: vi.fn(async () => undefined),
      persistDrawerTabOrder: vi.fn(),
      persistTerminalVisibility: vi.fn(),
      saveTerminalName: vi.fn(),
      setShellMode: vi.fn(),
      observeTerminalDrawer: vi.fn(),
      disconnectTerminalDrawerSize: vi.fn(),
      exitMobileTerminalFocus: vi.fn(),
      showToast: vi.fn(),
    };
  const drawer = createTerminalDrawerController({
    ...bindings,
    ...calls,
    project: () => project,
    aiToolLabel: (tool) => tool,
    settleTerminalDrawerGeometry: vi.fn(),
    terminalSession: () => undefined,
    currentDrawerTabIds: tabIds,
    activateOpenProject: () => undefined,
    refreshActivatedProject: vi.fn(async () => undefined),
    refreshProject: vi.fn(async () => undefined),
    refreshCommands: vi.fn(async () => undefined),
  });
  return { drawer, bindings, calls };
}

describe('terminal drawer owner (HS2-BSSWGV)', () => {
  it('messages only a live, capable connection in the selected project and explains delivery failures', async () => {
    const { drawer, bindings, calls } = owner();
    bindings.terminalGroups.value = [{ projectId: 'a', sessions: [{ id: 't1' }] } as unknown as TerminalDashboardGroup];
    const connection = {
      id: 't1',
      tool: 'claude',
      project: project.root,
      role: 'main',
      busy: false,
      actions: ['message'],
    };
    fetchMock.mockImplementation(async (input) =>
      (typeof input === 'string' ? input : input instanceof URL ? input.href : input.url).endsWith('/connections')
        ? new Response(JSON.stringify([connection]), { status: 200 })
        : new Response(JSON.stringify({ delivered: true, mechanism: 'claude-inbox', confirmed: false }), {
            status: 200,
          }),
    );
    await drawer.messageTerminalSession('t1', 'Continue');
    expect(fetchMock).toHaveBeenLastCalledWith(
      '/api/a/connections/t1/message',
      expect.objectContaining({ method: 'POST', body: '{"text":"Continue"}' }),
    );
    expect(calls.showToast).toHaveBeenLastCalledWith('Message delivered to the AI session.');

    fetchMock.mockClear();
    fetchMock.mockResolvedValueOnce(
      new Response(JSON.stringify([{ ...connection, project: '/other' }]), { status: 200 }),
    );
    await drawer.messageTerminalSession('t1', 'Wrong project');
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(calls.showToast).toHaveBeenLastCalledWith('This AI session cannot receive messages right now.');

    fetchMock.mockClear();
    fetchMock.mockResolvedValueOnce(new Response(JSON.stringify([connection]), { status: 200 }));
    fetchMock.mockResolvedValueOnce(new Response(JSON.stringify({ error: 'unsupported' }), { status: 422 }));
    await drawer.messageTerminalSession('t1', 'Retry');
    expect(calls.showToast).toHaveBeenLastCalledWith('This AI tool or session cannot receive messages yet.');

    fetchMock.mockClear();
    fetchMock.mockResolvedValueOnce(new Response(JSON.stringify([connection]), { status: 200 }));
    fetchMock.mockResolvedValueOnce(new Response(JSON.stringify({ error: 'no_session_inbox' }), { status: 409 }));
    await drawer.messageTerminalSession('t1', 'Retry');
    expect(calls.showToast).toHaveBeenLastCalledWith(
      'This AI session has no message inbox. Restart the session and try again.',
    );
  });
  it('opens, mounts, and remembers the drawer, then unmounts it after the close transition', () => {
    const { drawer, bindings, calls } = owner();
    drawer.setTerminalDrawerVisible(true);
    expect(bindings.terminalDrawerVisible.value).toBe(true);
    expect(bindings.terminalDrawerMounted.value).toBe(true);
    expect(bindings.terminalDrawerTransitioning.value).toBe(true);
    expect(stored.get('hotsheet.terminals.drawer-open')).toBe('true');
    expect(calls.refreshTerminalDashboard).toHaveBeenCalledTimes(1);
    drawer.setTerminalDrawerVisible(false);
    expect(calls.exitMobileTerminalFocus).toHaveBeenCalled();
    expect(calls.disconnectTerminalDrawerSize).toHaveBeenCalled();
    vi.advanceTimersByTime(220);
    expect(bindings.terminalDrawerTransitioning.value).toBe(false);
    expect(bindings.terminalDrawerMounted.value).toBe(false);
  });

  it('selects a drawer chat, binds its conversation, and requests terminal focus for a terminal', () => {
    const { drawer, bindings } = owner();
    bindings.terminalDrawerChatsByProject.value = {
      a: [{ id: 'ai-chat:c', connectionId: 'c', tool: 'claude', name: 'chat' }],
    };
    drawer.selectDrawerItem('ai-chat:c');
    expect(bindings.conversationConnectionId.value).toBe('c');
    expect(drawer.pendingTerminalFocus).toBeUndefined();
    drawer.selectDrawerItem('t1');
    expect(bindings.terminalDrawerSelected.value).toBe('t1');
    expect(stored.get('hotsheet.project.a.terminal-drawer-selection')).toBe('t1');
    expect(drawer.pendingTerminalFocus).toEqual({ projectId: 'a', terminalId: 't1' });
  });

  it('serializes terminal creation so a second request runs after the first, even when it fails', async () => {
    const { drawer } = owner(),
      order: string[] = [];
    let release: () => void = () => undefined;
    const first = drawer.queueTerminalCreate(
        () =>
          new Promise<void>((resolve, reject) => {
            order.push('first');
            release = () => {
              reject(new Error('boom'));
              resolve();
            };
          }),
      ),
      second = drawer.queueTerminalCreate(async () => {
        order.push('second');
      });
    await vi.advanceTimersByTimeAsync(0);
    expect(order).toEqual(['first']);
    release();
    await expect(first).rejects.toThrow('boom');
    await second;
    expect(order).toEqual(['first', 'second']);
  });

  it('closes a selected terminal tab, deletes it, and moves the selection to the next tab', async () => {
    const { drawer, bindings, calls } = owner();
    bindings.terminalGroups.value = [
      { projectId: 'a', sessions: [{ id: 't1' }, { id: 't2' }] } as unknown as TerminalDashboardGroup,
    ];
    bindings.terminalDrawerSelected.value = 't1';
    fetchMock.mockImplementation(() => json({}));
    await drawer.closeDrawerTabIds(['t1']);
    expect(fetchMock.mock.calls.map(([url]) => url as string)).toEqual(['/api/a/terminals/t1']);
    expect(calls.persistDrawerTabOrder).toHaveBeenCalledWith('a', ['t2', 'ai-chat:c']);
    expect(bindings.terminalDrawerSelected.value).toBe('t2');
    // Closing from the tab bar keeps focus on the tabs instead of queuing input focus.
    expect(drawer.pendingTerminalFocus).toBeUndefined();
  });

  it('reports a failed terminal delete and keeps the tab', async () => {
    const { drawer, bindings, calls } = owner();
    bindings.terminalGroups.value = [{ projectId: 'a', sessions: [{ id: 't1' }] } as unknown as TerminalDashboardGroup];
    bindings.terminalDrawerSelected.value = 't1';
    fetchMock.mockImplementation(() => Promise.resolve(new Response('nope', { status: 500 })));
    await drawer.closeDrawerTabIds(['t1']);
    expect(bindings.terminalDashboardMessage.value).not.toBe('');
    expect(bindings.terminalDashboardLoading.value).toBe(false);
    expect(calls.persistDrawerTabOrder).not.toHaveBeenCalled();
    expect(bindings.terminalDrawerSelected.value).toBe('t1');
  });
});

describe('drawer add and remove transitions (HS2-AQ40G7)', () => {
  type Bindings = ReturnType<typeof owner>['bindings'];
  const sessions = (...ids: string[]) =>
    [{ projectId: 'a', sessions: ids.map((id) => ({ id })) }] as unknown as TerminalDashboardGroup[];
  const projectA = <T>(record: Partial<Record<string, T[]>>): T[] => record.a ?? [];
  /** Every view the drawer would render, recorded on each signal write, collapsed to its changes. */
  function recordViews(bindings: Bindings) {
    const views: string[] = [];
    const dispose = effect(() => {
      const live = new Set([
          ...(bindings.terminalGroups.value.find((group) => group.projectId === 'a')?.sessions ?? []).map(
            (session) => session.id,
          ),
          ...projectA(bindings.terminalDrawerChatsByProject.value).map((chat) => chat.id),
        ]),
        view = drawerSelectionView(
          bindings.terminalDrawerSelected.value,
          live,
          projectA(bindings.terminalDrawerPendingByProject.value),
        ),
        label = view.kind === 'live' ? view.id : view.kind === 'pending' ? `pending:${view.tab.name}` : 'grid';
      if (views.at(-1) !== label) views.push(label);
    });
    return { views, dispose };
  }
  function gate<T>() {
    let release: (value: T) => void = () => undefined;
    const promise = new Promise<T>((resolve) => {
      release = resolve;
    });
    return { promise, release };
  }

  it('selects a pending tab at once and replaces it in place with the created terminal', async () => {
    const { drawer, bindings, calls } = owner();
    bindings.terminalGroups.value = sessions('t1');
    bindings.terminalDrawerSelected.value = 't1';
    const created = gate<Response>();
    fetchMock.mockImplementation(() => created.promise);
    calls.refreshTerminalDashboard.mockImplementation(async () => {
      bindings.terminalGroups.value = sessions('t1', 't3');
    });
    const { views, dispose } = recordViews(bindings);
    const run = drawer.createProjectTerminal({ tool: 'claude', model: 'm', effort: 'e' });
    // Requested: the placeholder is selected before the server answers.
    expect(bindings.terminalDrawerPendingByProject.value.a).toEqual([
      { id: expect.stringMatching(/^pending:/), name: 'claude', kind: 'terminal' },
    ]);
    expect(bindings.terminalDrawerSelected.value).toMatch(/^pending:/);
    // A placeholder is never remembered as the drawer selection.
    expect(stored.get('hotsheet.project.a.terminal-drawer-selection')).toBeUndefined();
    created.release(
      new Response(JSON.stringify({ id: 't3', alive: true, busy: false, cwd: '/work/a' }), { status: 201 }),
    );
    await run;
    expect(bindings.terminalDrawerSelected.value).toBe('t3');
    expect(bindings.terminalDrawerPendingByProject.value.a).toEqual([]);
    expect(drawer.pendingTerminalFocus).toEqual({ projectId: 'a', terminalId: 't3' });
    expect(views).toEqual(['t1', 'pending:claude', 't3']);
    dispose();
  });

  it('removes a failed placeholder, returns to the previous tab, and reports why', async () => {
    const { drawer, bindings, calls } = owner();
    bindings.terminalGroups.value = sessions('t1');
    bindings.terminalDrawerSelected.value = 't1';
    fetchMock.mockImplementation(() => Promise.resolve(new Response('no pty', { status: 500 })));
    const { views, dispose } = recordViews(bindings);
    await drawer.createProjectTerminal();
    expect(bindings.terminalDrawerPendingByProject.value.a).toEqual([]);
    expect(bindings.terminalDrawerSelected.value).toBe('t1');
    expect(bindings.terminalDashboardMessage.value).not.toBe('');
    expect(calls.showToast).toHaveBeenCalledWith(expect.stringMatching(/^Terminal could not start: /));
    expect(views).toEqual(['t1', 'pending:Terminal', 't1']);
    dispose();
  });

  it('leaves the selection alone when the person moved to another tab while the terminal started', async () => {
    const { drawer, bindings, calls } = owner();
    bindings.terminalGroups.value = sessions('t1', 't2');
    bindings.terminalDrawerSelected.value = 't1';
    const created = gate<Response>();
    fetchMock.mockImplementation(() => created.promise);
    calls.refreshTerminalDashboard.mockImplementation(async () => {
      bindings.terminalGroups.value = sessions('t1', 't2', 't3');
    });
    const run = drawer.createProjectTerminal();
    drawer.selectDrawerItem('t2');
    created.release(
      new Response(JSON.stringify({ id: 't3', alive: true, busy: false, cwd: '/work/a' }), { status: 201 }),
    );
    await run;
    expect(bindings.terminalDrawerSelected.value).toBe('t2');
    expect(bindings.terminalDrawerPendingByProject.value.a).toEqual([]);
  });

  it('swaps a pending AI chat for its chat tab in one update and restores the selection on failure', async () => {
    const { drawer, bindings } = owner();
    bindings.terminalGroups.value = sessions('t1');
    bindings.terminalDrawerSelected.value = 't1';
    const created = gate<Response>();
    fetchMock.mockImplementation(() => created.promise);
    const { views, dispose } = recordViews(bindings);
    const run = drawer.createDrawerAIChat({ tool: 'claude', model: 'm', effort: 'e' }, { connectionId: 'c2' });
    expect(bindings.terminalDrawerSelected.value).toMatch(/^pending:/);
    created.release(
      new Response(JSON.stringify({ id: 'c2', tool: 'claude', project: '/work/a', role: 'main', busy: false }), {
        status: 200,
      }),
    );
    await run;
    expect(bindings.terminalDrawerSelected.value).toBe('ai-chat:c2');
    expect(bindings.terminalDrawerPendingByProject.value.a).toEqual([]);
    expect(views).toEqual(['t1', 'pending:claude chat', 'ai-chat:c2']);

    fetchMock.mockImplementation(() => Promise.resolve(new Response('down', { status: 502 })));
    await drawer.createDrawerAIChat({ tool: 'claude', model: 'm', effort: 'e' }, { connectionId: 'c3' });
    expect(bindings.terminalDrawerSelected.value).toBe('ai-chat:c2');
    expect(bindings.terminalDrawerPendingByProject.value.a).toEqual([]);
    expect(bindings.error.value).not.toBe('');
    expect(views.slice(-2)).toEqual(['pending:claude chat', 'ai-chat:c2']);
    dispose();
  });

  it('selects the neighbor before deleting the selected tab, and the grid only after the last one', async () => {
    let order = ['t1', 't2'];
    const { drawer, bindings, calls } = owner(() => order);
    bindings.terminalGroups.value = sessions('t1', 't2');
    bindings.terminalDrawerSelected.value = 't1';
    const deleted = gate<Response>();
    fetchMock.mockImplementation(() => deleted.promise);
    calls.refreshTerminalDashboard.mockImplementation(async () => {
      bindings.terminalGroups.value = sessions('t2');
    });
    const { views, dispose } = recordViews(bindings);
    const run = drawer.closeDrawerTabIds(['t1']);
    // The neighbor is selected while the delete is still in flight.
    expect(bindings.terminalDrawerSelected.value).toBe('t2');
    deleted.release(new Response(null, { status: 204 }));
    await run;
    expect(views).toEqual(['t1', 't2']);
    order = ['t2'];

    // Closing the last tab keeps the existing empty behavior: the Project grid.
    fetchMock.mockImplementation(() => Promise.resolve(new Response(null, { status: 204 })));
    calls.refreshTerminalDashboard.mockImplementation(async () => {
      bindings.terminalGroups.value = sessions();
    });
    await drawer.closeDrawerTabIds(['t2']);
    expect(bindings.terminalDrawerSelected.value).toBe('grid');
    expect(views).toEqual(['t1', 't2', 'grid']);
    dispose();
  });

  it('closes a selected AI chat straight to its neighbor', async () => {
    const { drawer, bindings } = owner();
    bindings.terminalGroups.value = sessions('t1', 't2');
    bindings.terminalDrawerChatsByProject.value = {
      a: [{ id: 'ai-chat:c', connectionId: 'c', tool: 'claude', name: 'chat' }],
    };
    bindings.terminalDrawerSelected.value = 'ai-chat:c';
    const { views, dispose } = recordViews(bindings);
    await drawer.closeDrawerTabIds(['ai-chat:c']);
    expect(fetchMock).not.toHaveBeenCalled();
    expect(bindings.terminalDrawerSelected.value).toBe('t2');
    expect(views).toEqual(['ai-chat:c', 't2']);
    dispose();
  });
});
