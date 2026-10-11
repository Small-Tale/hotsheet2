import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import type { Project } from '../interactions/types';
import { TERMINAL_DASHBOARD_VISIBILITY_SCOPE } from '../terminal-visibility';
import { createTerminalDashboardController } from './terminal-dashboard';

const project = (id: string): Project => ({
  id,
  name: id,
  root: `/work/${id}`,
  apiPath: `/api/${id}`,
  stores: [],
  compatibility: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
});
const fetchMock = vi.fn<typeof fetch>();
const stored = new Map<string, string>();

beforeEach(() => {
  vi.useFakeTimers();
  fetchMock.mockReset();
  stored.clear();
  vi.stubGlobal('fetch', fetchMock);
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
  });
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function owner(projects = signal([project('a')])) {
  const terminalGroups = signal<TerminalDashboardGroup[]>([]),
    terminalDashboardLoading = signal(false),
    terminalDashboardMessage = signal(''),
    terminalVisibility = signal({ activeGroupId: 'all', groups: [] } as never),
    refreshDriveConnections = vi.fn(async () => undefined),
    reconcileTerminalNames = vi.fn(),
    showToast = vi.fn(),
    error = signal('');
  const controller = createTerminalDashboardController({
    projects,
    selectedProjectId: () => 'a',
    terminalGroups,
    terminalDashboardLoading,
    terminalDashboardMessage,
    terminalVisibility,
    terminalVisibilityDialogScope: () => undefined,
    terminalVisibilityFilter: () => [],
    terminalDrawerOrderByProject: signal({}),
    terminalDrawerChatsByProject: () => ({ a: [{ id: 'ai-chat:c', connectionId: 'c', tool: 'claude', name: 'chat' }] }),
    terminalNames: () => ({ 'a:t2': 'Renamed' }),
    hasDriveConnections: () => false,
    refreshDriveConnections,
    reconcileTerminalNames,
    workspaceTerminalGroups: () => terminalGroups.value,
    aiToolLabel: (tool) => tool,
    showToast,
    error,
  });
  return {
    controller,
    terminalGroups,
    terminalDashboardLoading,
    terminalDashboardMessage,
    refreshDriveConnections,
    reconcileTerminalNames,
    showToast,
    error,
  };
}

const terminals = (ids: string[]) =>
  new Response(JSON.stringify(ids.map((id) => ({ id, alive: true, busy: false, cwd: '/work/a' }))));

describe('terminal dashboard owner (HS2-ZJ67VE)', () => {
  it('loads owned terminals with local names, keeps the last snapshot on failure, and persists tab order', async () => {
    const state = owner();
    fetchMock.mockResolvedValueOnce(terminals(['t1', 't2']));
    await state.controller.refreshTerminalDashboard();
    expect(state.terminalGroups.value[0].sessions.map((session) => [session.id, session.title])).toEqual([
      ['t1', expect.any(String)],
      ['t2', 'Renamed'],
    ]);
    expect(state.terminalDashboardLoading.value).toBe(false);
    expect(state.reconcileTerminalNames).toHaveBeenCalledTimes(1);
    expect(state.controller.terminalSession('a:t2')?.id).toBe('t2');
    // A failed refresh is not a resumed session: the previous snapshot stays.
    fetchMock.mockRejectedValueOnce(new Error('offline'));
    await state.controller.refreshTerminalDashboard(true);
    expect(state.terminalGroups.value[0].sessions).toHaveLength(2);
    // Drawer order is remembered across a refresh and interleaves AI chats.
    state.controller.persistDrawerTabOrder('a', ['t2', 'ai-chat:c', 't1']);
    expect(state.terminalGroups.value[0].sessions.map((session) => session.id)).toEqual(['t2', 't1']);
    expect(state.controller.currentDrawerTabIds('a')).toEqual(['t2', 'ai-chat:c', 't1']);
    fetchMock.mockResolvedValueOnce(terminals(['t1', 't2']));
    await state.controller.refreshTerminalDashboard();
    expect(state.terminalGroups.value[0].sessions.map((session) => session.id)).toEqual(['t2', 't1']);
  });

  it('reports an empty load, ignores a superseded loud refresh, and resolves visibility scopes', async () => {
    const state = owner();
    let release!: (response: Response) => void;
    fetchMock.mockImplementationOnce(() => new Promise((resolve) => (release = resolve)));
    const stale = state.controller.refreshTerminalDashboard();
    fetchMock.mockRejectedValueOnce(new Error('offline'));
    await state.controller.refreshTerminalDashboard();
    expect(state.terminalDashboardMessage.value).toBe('Terminal snapshots could not be loaded.');
    release(terminals(['late']));
    await stale;
    expect(state.terminalGroups.value).toEqual([]);
    expect(state.controller.terminalVisibilityScopeFor({ closest: () => null } as unknown as Element)).toBe(
      TERMINAL_DASHBOARD_VISIBILITY_SCOPE,
    );
  });

  it('clears a stopped terminal halt and dispose cancels the AI-connection re-derive timer', async () => {
    const state = owner();
    fetchMock.mockResolvedValueOnce(
      new Response(JSON.stringify([{ id: 't1', alive: true, busy: false, cwd: '/work/a', halt: { at: 'x' } }])),
    );
    await state.controller.refreshTerminalDashboard();
    fetchMock.mockResolvedValueOnce(new Response(null, { status: 204 })).mockResolvedValueOnce(terminals(['t1']));
    await state.controller.clearTerminalHalt('a:t1');
    expect(state.showToast).toHaveBeenCalledWith('Stopped state cleared.');
    fetchMock.mockRejectedValueOnce(new Error('denied'));
    state.terminalGroups.value = [
      {
        ...state.terminalGroups.value[0],
        sessions: [{ ...state.terminalGroups.value[0].sessions[0], halt: { at: 'y' } }],
      },
    ] as never;
    await state.controller.clearTerminalHalt('a:t1');
    expect(state.error.value).toBe('denied');
    // An AI terminal inside its connection grace period schedules a local re-derive; dispose cancels it.
    fetchMock.mockResolvedValueOnce(
      new Response(
        JSON.stringify([{ id: 'ai1', kind: 'ai', tool: 'claude', alive: true, busy: false, cwd: '/work/a' }]),
      ),
    );
    await state.controller.refreshTerminalDashboard();
    const scheduled = vi.getTimerCount();
    state.controller.dispose();
    expect(vi.getTimerCount()).toBe(scheduled - 1);
  });
});
