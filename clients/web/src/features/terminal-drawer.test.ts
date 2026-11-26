import { effect, signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { ToolConnection } from '../api';
import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import { drawerSelectionView, type PendingDrawerTab } from '../drawer-tab-order';
import type { Project } from '../interactions/types';
import { handBackOperationId } from '../orchestration-hand-back';
import type { DrawerAIChat } from '../project-drive';
import { createTerminalDrawerController, DRAWER_APPEAR_TIMEOUT_MS, DRAWER_CREATE_TIMEOUT_MS } from './terminal-drawer';

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

  it('opens and settles the drawer even when storage refuses the remembered choice (HS2-N7QZJ4)', () => {
    vi.stubGlobal('localStorage', {
      getItem: () => null,
      setItem: () => {
        throw new DOMException('quota', 'QuotaExceededError');
      },
    });
    const { drawer, bindings, calls } = owner();
    expect(() => {
      drawer.setTerminalDrawerVisible(true);
    }).not.toThrow();
    expect(bindings.terminalDrawerVisible.value).toBe(true);
    expect(calls.refreshTerminalDashboard).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(220);
    expect(bindings.terminalDrawerTransitioning.value).toBe(false);
    drawer.setTerminalDrawerVisible(false);
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

describe('hand back to the orchestrator (HS2-KR28FS)', () => {
  const orchestrated = (owner: 'orchestrator' | 'human') =>
    [
      {
        projectId: 'a',
        sessions: [
          { id: 't1', orchestrator: { checkout_id: 'c1', operation_id: 'op-1', project: 'P', owner, paused: false } },
        ],
      },
    ] as unknown as TerminalDashboardGroup[];
  const sent = () =>
    fetchMock.mock.calls.map(
      ([, init]) => JSON.parse(typeof init?.body === 'string' ? init.body : '{}') as Record<string, unknown>,
    );

  it('continues a permission request once answered, starts a new one after a denial, and explains refusals', async () => {
    const { drawer, bindings, calls } = owner();
    // The orchestrator's own session is never handed back.
    bindings.terminalGroups.value = orchestrated('orchestrator');
    await drawer.handBackHostedSession('t1');
    expect(fetchMock).not.toHaveBeenCalled();

    bindings.terminalGroups.value = orchestrated('human');
    const failure = (code: string) =>
      Promise.resolve(new Response(JSON.stringify({ code, request_id: 'req-1' }), { status: 409 }));
    fetchMock.mockImplementationOnce(() => failure('permission_required'));
    await drawer.handBackHostedSession('t1');
    expect(calls.showToast).toHaveBeenLastCalledWith(
      'Handing back needs permission to start a session. It finishes once the request is allowed.',
    );
    fetchMock.mockImplementationOnce(() => json({ refused: 'session_mid_turn' }));
    await drawer.handBackHostedSession('t1');
    expect(calls.showToast).toHaveBeenLastCalledWith(
      'The session is in the middle of a turn. Hand it back when it is idle.',
    );
    fetchMock.mockImplementationOnce(() => failure('permission_denied'));
    await drawer.handBackHostedSession('t1');
    expect(calls.showToast).toHaveBeenLastCalledWith(
      'Permission to start a session was denied. The session stays yours.',
    );
    fetchMock.mockImplementationOnce(() => json({}));
    await drawer.handBackHostedSession('t1');
    expect(calls.showToast).toHaveBeenLastCalledWith('Handed back to the orchestrator.');
    fetchMock.mockImplementationOnce(() => json({ refused: 'session_owner_remote' }));
    await drawer.handBackHostedSession('t1');
    expect(calls.showToast).toHaveBeenLastCalledWith(
      'Another machine orchestrates this session. Hand it back from there.',
    );

    const bodies = sent();
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/a/checkouts/c1/target-sessions');
    expect(bodies[0]).toMatchObject({ operation_id: 'op-1', hand_back: true, actor: { role: 'human' } });
    const ids = bodies.map((body) => body.hand_back_operation_id);
    // Waiting on permission, then mid-turn, then denied: one request; after the denial a new one,
    // kept through the completed hand-back; after it, a new one again.
    expect(ids[1]).toBe(ids[0]);
    expect(ids[2]).toBe(ids[0]);
    expect(ids[3]).not.toBe(ids[0]);
    expect(ids[4]).not.toBe(ids[3]);
    for (const id of ids) expect(id).toMatch(/^hand-back-[A-Za-z0-9-]+$/u);
    expect(calls.refreshTerminalDashboard).toHaveBeenCalledTimes(5);
  });
});

describe('a hand-back finishes when its permission is answered (HS2-BPTQ5X)', () => {
  const groups = [
    {
      projectId: 'a',
      sessions: [
        {
          id: 't1',
          orchestrator: { checkout_id: 'c1', operation_id: 'op-1', project: 'P', owner: 'human', paused: false },
        },
      ],
    },
  ] as unknown as TerminalDashboardGroup[];
  const required = () =>
    Promise.resolve(
      new Response(JSON.stringify({ code: 'permission_required', request_id: 'req-1' }), { status: 409 }),
    );
  const bodies = () =>
    fetchMock.mock.calls.map(
      ([, init]) => JSON.parse(typeof init?.body === 'string' ? init.body : '{}') as Record<string, unknown>,
    );
  const ask = (operation: unknown) => ({
    request_id: 'req-1',
    operation_id: String(operation),
    owner_checkout_id: 'o',
  });

  it('sends it again under the same id once the request is answered, and reports a denial', async () => {
    const { drawer, bindings, calls } = owner();
    bindings.terminalGroups.value = groups;
    fetchMock.mockImplementationOnce(required);
    await drawer.handBackHostedSession('t1');
    const id = bodies()[0]?.hand_back_operation_id;
    // Pending: nothing is sent again; seen pending, then answered: sent once more, same id.
    drawer.continueHandBacks([ask(id)]);
    drawer.continueHandBacks([ask(id)]);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    fetchMock.mockImplementationOnce(() => json({}));
    drawer.continueHandBacks([]);
    await vi.waitFor(() => {
      expect(calls.showToast).toHaveBeenLastCalledWith('Handed back to the orchestrator.');
    });
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(bodies()[1]?.hand_back_operation_id).toBe(id);
    // Done: a later change of the inbox sends nothing.
    drawer.continueHandBacks([]);
    expect(fetchMock).toHaveBeenCalledTimes(2);

    // A new hand-back asks again under a new id; a Deny is reported and ends it.
    fetchMock.mockImplementationOnce(required);
    await drawer.handBackHostedSession('t1');
    const second = bodies()[2]?.hand_back_operation_id;
    expect(second).not.toBe(id);
    drawer.continueHandBacks([ask(second)]);
    fetchMock.mockImplementationOnce(() =>
      Promise.resolve(new Response(JSON.stringify({ code: 'permission_denied' }), { status: 403 })),
    );
    drawer.continueHandBacks([]);
    await vi.waitFor(() => {
      expect(calls.showToast).toHaveBeenLastCalledWith(
        'Permission to start a session was denied. The session stays yours.',
      );
    });
    drawer.continueHandBacks([]);
    expect(fetchMock).toHaveBeenCalledTimes(4);
  });

  it('finishes a hand-back whose request reached the inbox before its own answer did', async () => {
    const { drawer, bindings, calls } = owner();
    bindings.terminalGroups.value = groups;
    // The inbox shows the request before the hand-back's own permission_required arrives.
    fetchMock.mockImplementationOnce(() => {
      drawer.continueHandBacks([ask('pending')]);
      return required();
    });
    await drawer.handBackHostedSession('t1');
    fetchMock.mockImplementationOnce(() => json({}));
    drawer.continueHandBacks([]);
    await vi.waitFor(() => {
      expect(calls.showToast).toHaveBeenLastCalledWith('Handed back to the orchestrator.');
    });
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('recovers a waiting hand-back after a reload and finishes it once on the answer', async () => {
    const { drawer, bindings, calls } = owner();
    bindings.terminalGroups.value = groups;
    // A fresh drawer remembers nothing; the inbox still holds the request it filed.
    const id = handBackOperationId('c1', 'op-1', 'beforereload');
    drawer.continueHandBacks([ask(id)]);
    expect(fetchMock).not.toHaveBeenCalled();
    fetchMock.mockImplementationOnce(() => json({}));
    drawer.continueHandBacks([]);
    await vi.waitFor(() => {
      expect(calls.showToast).toHaveBeenLastCalledWith('Handed back to the orchestrator.');
    });
    expect(bodies()[0]).toMatchObject({ operation_id: 'op-1', hand_back: true, hand_back_operation_id: id });
    drawer.continueHandBacks([]);
    expect(fetchMock).toHaveBeenCalledTimes(1);
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

  it('keeps a failed placeholder with recovery, then closes back to the previous tab', async () => {
    const { drawer, bindings, calls } = owner();
    bindings.terminalGroups.value = sessions('t1');
    bindings.terminalDrawerSelected.value = 't1';
    fetchMock.mockImplementation(() => Promise.resolve(new Response('no pty', { status: 500 })));
    const { views, dispose } = recordViews(bindings);
    await drawer.createProjectTerminal();
    const failed = bindings.terminalDrawerPendingByProject.value.a[0];
    expect(failed.failure).toBe('a: 500');
    expect(bindings.terminalDrawerSelected.value).toBe(failed.id);
    expect(bindings.terminalDashboardMessage.value).not.toBe('');
    expect(calls.showToast).not.toHaveBeenCalled();
    drawer.closeFailedDrawerTab(failed.id);
    expect(bindings.terminalDrawerPendingByProject.value.a).toEqual([]);
    expect(bindings.terminalDrawerSelected.value).toBe('t1');
    expect(views).toEqual(['t1', 'pending:Terminal', 't1']);
    dispose();
  });

  it('times out a hung create, releases the queue, and retries from the failed tab (HS2-S2T4VJ)', async () => {
    const { drawer, bindings, calls } = owner();
    bindings.terminalGroups.value = sessions('t1');
    bindings.terminalDrawerSelected.value = 't1';
    let aborted = false;
    fetchMock.mockImplementationOnce(
      (_url, init) =>
        new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener('abort', () => {
            aborted = true;
            reject(new DOMException('Aborted', 'AbortError'));
          });
        }),
    );
    fetchMock.mockImplementation(() => json({ id: 't2', alive: true, busy: false, cwd: '/work/a' }));
    calls.refreshTerminalDashboard.mockImplementation(async () => {
      bindings.terminalGroups.value = sessions('t1', 't2');
      bindings.terminalDashboardLoading.value = false;
    });
    const first = drawer.createProjectTerminal();
    const second = drawer.createProjectTerminal();
    await vi.advanceTimersByTimeAsync(DRAWER_CREATE_TIMEOUT_MS);
    await Promise.all([first, second]);
    expect(aborted).toBe(true);
    expect(fetchMock).toHaveBeenCalledTimes(2);
    const failed = bindings.terminalDrawerPendingByProject.value.a[0];
    expect(failed.failure).toContain('15 seconds on a');
    expect(bindings.terminalDashboardLoading.value).toBe(false);
    expect(bindings.terminalDrawerPendingByProject.value.a).toHaveLength(1);
    drawer.retryFailedDrawerTab(failed.id);
    await vi.advanceTimersByTimeAsync(0);
    expect(fetchMock).toHaveBeenCalledTimes(3);
    expect(JSON.parse(fetchMock.mock.calls[0][1]?.body as string).id).toBe(
      JSON.parse(fetchMock.mock.calls[2][1]?.body as string).id,
    );
    expect(bindings.terminalDrawerPendingByProject.value.a).toEqual([]);
  });

  it('closes a second failed retry back to the original live tab (HS2-S2T4VJ)', async () => {
    const { drawer, bindings } = owner();
    bindings.terminalGroups.value = sessions('t1');
    bindings.terminalDrawerSelected.value = 't1';
    fetchMock.mockImplementation(() => Promise.resolve(new Response('unavailable', { status: 503 })));
    await drawer.createProjectTerminal();
    const first = bindings.terminalDrawerPendingByProject.value.a[0];
    drawer.retryFailedDrawerTab(first.id);
    await vi.advanceTimersByTimeAsync(0);
    const second = bindings.terminalDrawerPendingByProject.value.a[0];
    expect(second.failure).toContain('503');
    drawer.closeFailedDrawerTab(second.id);
    expect(bindings.terminalDrawerSelected.value).toBe('t1');
    expect(bindings.terminalDrawerPendingByProject.value.a).toEqual([]);
  });

  it('fails a created terminal that never appears, then accepts a later stream update (HS2-S2T4VJ)', async () => {
    const { drawer, bindings, calls } = owner();
    bindings.terminalGroups.value = sessions('t1');
    fetchMock.mockImplementation(() => json({ id: 't3', alive: true, busy: false, cwd: '/work/a' }));
    const run = drawer.createProjectTerminal();
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.refreshTerminalDashboard).toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(DRAWER_APPEAR_TIMEOUT_MS);
    await run;
    expect(bindings.terminalDrawerPendingByProject.value.a[0]?.failure).toContain('did not appear');
    drawer.closeFailedDrawerTab(bindings.terminalDrawerPendingByProject.value.a[0].id);

    const later = drawer.createProjectTerminal();
    await vi.advanceTimersByTimeAsync(0);
    bindings.terminalGroups.value = sessions('t1', 't3');
    await later;
    expect(bindings.terminalDrawerPendingByProject.value.a).toEqual([]);
    expect(bindings.terminalDrawerSelected.value).toBe('t3');
  });

  it('releases the create queue when dashboard refresh hangs after a successful POST (HS2-S2T4VJ)', async () => {
    const { drawer, bindings, calls } = owner();
    bindings.terminalGroups.value = sessions('t1');
    fetchMock
      .mockImplementationOnce(() => json({ id: 't3', alive: true, busy: false, cwd: '/work/a' }))
      .mockImplementationOnce(() => Promise.resolve(new Response('unavailable', { status: 503 })));
    calls.refreshTerminalDashboard.mockImplementation(() => new Promise<undefined>(() => undefined));

    const first = drawer.createProjectTerminal();
    const second = drawer.createProjectTerminal();
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.refreshTerminalDashboard).toHaveBeenCalledTimes(1);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(DRAWER_APPEAR_TIMEOUT_MS);
    await Promise.all([first, second]);

    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(bindings.terminalDrawerPendingByProject.value.a).toHaveLength(2);
    expect(bindings.terminalDrawerPendingByProject.value.a[0].failure).toContain('did not appear');
    expect(bindings.terminalDrawerPendingByProject.value.a[1].failure).toContain('503');
    expect(bindings.terminalDashboardLoading.value).toBe(false);
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
    const failed = bindings.terminalDrawerPendingByProject.value.a[0];
    expect(bindings.terminalDrawerSelected.value).toBe(failed.id);
    expect(failed.failure).toBe('a: 502');
    expect(bindings.error.value).not.toBe('');
    drawer.closeFailedDrawerTab(failed.id);
    expect(bindings.terminalDrawerSelected.value).toBe('ai-chat:c2');
    expect(bindings.terminalDrawerPendingByProject.value.a).toEqual([]);
    expect(views.slice(-2)).toEqual(['pending:claude chat', 'ai-chat:c2']);
    dispose();
  });

  it('times out a hung AI chat create and clears its drive-pending state (HS2-S2T4VJ)', async () => {
    const { drawer, bindings } = owner();
    fetchMock.mockImplementation(() => new Promise<Response>(() => undefined));
    const run = drawer.createDrawerAIChat({ tool: 'claude', model: 'm', effort: 'e' }, { connectionId: 'stuck' });
    expect(bindings.drivePendingByProject.value.a).toBe(true);
    await vi.advanceTimersByTimeAsync(DRAWER_CREATE_TIMEOUT_MS);
    await run;
    expect(bindings.drivePendingByProject.value.a).toBe(false);
    expect(bindings.terminalDrawerPendingByProject.value.a[0].failure).toContain('15 seconds on a');
    expect(bindings.error.value).toContain('15 seconds');
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
