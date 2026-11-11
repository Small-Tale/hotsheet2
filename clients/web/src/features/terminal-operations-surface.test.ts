import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { CheckoutTicketCounts, LiveStoreState } from '../api';
import { GRID_SIDEBAR_PAGE_STORAGE_KEY } from '../components/terminal-operations-sidebar';
import type { Project } from '../interactions/types';
import { createLiveStateStore } from '../live-state';
import { createTerminalOperationsSurface } from './terminal-operations-surface';

const MINUTE = 60_000;
const NOW = 1_800_000_000_000; // a whole minute

/** A snapshot in the server's exact shape (HS2-JTTQMY); synthetic names only. */
function store(id: string, checkouts: string[], overrides: Partial<LiveStoreState> = {}): LiveStoreState {
  return {
    store: id,
    root: `/work/${id}.hs2`,
    name: id,
    checkouts,
    epoch_ms: 1,
    revision: 1,
    state: 'active',
    activity: {
      word: 'active',
      active_since_ms: NOW - 10 * MINUTE,
      stopped_at_ms: null,
      last_progress_ms: NOW - 2 * MINUTE,
      idle_deadline_ms: null,
      stalled: [],
      halted: [],
      causes: [],
    },
    claims: [],
    workers: { live: 1, expected: 1 },
    waiting: { count: 0, tickets: [] },
    rates: {
      tokens: { window_minutes: 1, minutes: [{ start_ms: NOW, tokens: 600 }], peak_per_minute: 1000 },
      tickets_updated: { window_minutes: 60, at_ms: [NOW - MINUTE], peak_per_hour: 10 },
    },
    ...overrides,
  };
}

function memoryStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
  };
}

function surfaceWith(options: Partial<Parameters<typeof createTerminalOperationsSurface>[0]> = {}) {
  return createTerminalOperationsSurface({
    partialSourcesByProject: signal({}),
    projects: signal([{ id: 'p1', name: 'One' } as Project, { id: 'p2', name: 'Two' } as Project]),
    projectTicketCounts: () => ({ completed_today: 0, started: 0 }) as CheckoutTicketCounts,
    selectedProjectId: signal('p1'),
    ticketRowsByProject: signal({}),
    tickets: signal([]),
    ...options,
  });
}

describe('grid sidebar live numbers and page (HS2-EZSQDF)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('projects each project its linked stores and counts a shared store once in All projects', () => {
    const liveStates = createLiveStateStore();
    liveStates.apply(store('s1', ['p1', 'p2']));
    liveStates.apply(store('s2', ['p2'], { state: 'may_be_stuck' }));
    const surface = surfaceWith({ liveStates });
    const props = surface.terminalOperationsSurfaceProps();
    expect(props.projects.map((item) => [item.id, item.live?.state, item.live?.tokensPerMinute])).toEqual([
      ['p1', 'active', 600],
      ['p2', 'may_be_stuck', 1200],
    ]);
    expect(props.allLive).toMatchObject({ state: 'may_be_stuck', tokensPerMinute: 1200, workersLive: 2 });
    surface.dispose();
  });

  it('leaves live numbers out before any store reports, and arms no clock', () => {
    const liveStates = createLiveStateStore();
    const surface = surfaceWith({ liveStates });
    const props = surface.terminalOperationsSurfaceProps();
    expect(props.projects.every((item) => item.live === undefined)).toBe(true);
    expect(props.allLive).toBeUndefined();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('decays rates on a local minute clock that re-renders without a request', () => {
    const fetchSpy = vi.spyOn(globalThis, 'fetch');
    const liveStates = createLiveStateStore();
    liveStates.apply(store('s1', ['p1']));
    const surface = surfaceWith({ liveStates });
    expect(surface.terminalOperationsSurfaceProps().projects[0].live?.tokensPerMinute).toBe(600);
    expect(vi.getTimerCount()).toBe(1);
    // One render arms one timer; renders in between do not stack timers.
    surface.terminalOperationsSurfaceProps();
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(MINUTE);
    expect(surface.terminalOperationsSurfaceProps().projects[0].live?.tokensPerMinute).toBe(0);
    expect(fetchSpy).not.toHaveBeenCalled();
    surface.dispose();
    expect(vi.getTimerCount()).toBe(0);
    // The clock stops when the last store is gone.
    const again = surfaceWith({ liveStates });
    again.terminalOperationsSurfaceProps();
    liveStates.reset();
    vi.advanceTimersByTime(MINUTE);
    expect(vi.getTimerCount()).toBe(0);
    fetchSpy.mockRestore();
  });

  it('starts on Completion, restores and persists the viewer page, and ignores unknown values', () => {
    expect(surfaceWith().terminalOperationsSurfaceProps().page).toBe('completion');
    const storage = memoryStorage({ [GRID_SIDEBAR_PAGE_STORAGE_KEY]: 'triage' });
    const surface = surfaceWith({ storage });
    expect(surface.terminalOperationsSurfaceProps().page).toBe('triage');
    surface.selectGridSidebarPage('gauges');
    expect(surface.terminalOperationsSurfaceProps().page).toBe('gauges');
    expect(storage.values.get(GRID_SIDEBAR_PAGE_STORAGE_KEY)).toBe('gauges');
    surface.selectGridSidebarPage('bogus');
    expect(surface.gridSidebarPage.value).toBe('completion');
    expect(
      surfaceWith({ storage: memoryStorage({ [GRID_SIDEBAR_PAGE_STORAGE_KEY]: 'x' }) }).gridSidebarPage.value,
    ).toBe('completion');
  });

  it('keeps working when storage throws', () => {
    const storage = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    const surface = surfaceWith({ storage });
    expect(surface.gridSidebarPage.value).toBe('completion');
    surface.selectGridSidebarPage('triage');
    expect(surface.gridSidebarPage.value).toBe('triage');
  });
});
