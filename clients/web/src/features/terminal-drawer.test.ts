import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { ToolConnection } from '../api';
import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
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

function owner() {
  const bindings = {
      projects: signal([project]),
      selectedProjectId: signal('a'),
      terminalGroups: signal<TerminalDashboardGroup[]>([]),
      terminalDrawerVisible: signal(false),
      terminalDrawerMounted: signal(false),
      terminalDrawerTransitioning: signal(false),
      terminalDrawerSelected: signal('grid'),
      terminalDrawerChatsByProject: signal<Record<string, DrawerAIChat[]>>({}),
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
    };
  const drawer = createTerminalDrawerController({
    ...bindings,
    ...calls,
    project: () => project,
    aiToolLabel: (tool) => tool,
    settleTerminalDrawerGeometry: vi.fn(),
    terminalSession: () => undefined,
    currentDrawerTabIds: () => ['t1', 't2', 'ai-chat:c'],
    activateOpenProject: () => undefined,
    refreshActivatedProject: vi.fn(async () => undefined),
    refreshProject: vi.fn(async () => undefined),
    refreshCommands: vi.fn(async () => undefined),
  });
  return { drawer, bindings, calls };
}

describe('terminal drawer owner (HS2-BSSWGV)', () => {
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
