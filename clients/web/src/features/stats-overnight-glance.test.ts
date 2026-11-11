import { signal } from 'kerfjs';
import { describe, expect, it, vi } from 'vitest';

import type { LiveHistoryList } from '../api';
import type { ProjectTabBarMode } from '../components/project-tab-bar';
import type { StatsDashboardState } from '../components/stats-dashboard';
import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import type { Project } from '../interactions/types';
import { createLiveStateStore } from '../live-state';
import { GLANCE_FIXTURE_NOW as NOW, glanceScenario } from '../ux-demo/live-state-fixtures';
import { createStatsGlanceController } from './stats-glance';

const HOUR = 3_600_000;
const project = (id: string, name: string) => ({ id, name, apiPath: '/api' }) as unknown as Project;

function setup(options: { mode?: ProjectTabBarMode; standbyAt?: number; fail?: boolean } = {}) {
  let now = NOW;
  const liveStates = createLiveStateStore();
  for (const state of glanceScenario()) liveStates.apply(state);
  const store = [...liveStates.states.peek().values()][0];
  const reads: number[] = [];
  const list = (since: number): LiveHistoryList => ({
    since_ms: since,
    until_ms: now,
    stores: [
      {
        store: store.store,
        name: store.name,
        checkouts: store.checkouts,
        segments: [{ state: 'active', start_ms: since, end_ms: now }],
        completions: [{ slug: 'HS2-1', at_ms: now - HOUR }],
      },
      // A store the scope does not include never shows.
      { store: 'closed', name: 'Closed', checkouts: ['nowhere'], segments: [], completions: [] },
    ],
  });
  const liveHistory = vi.fn((since: number) => {
    reads.push(since);
    return options.fail ? Promise.reject(new Error('offline')) : Promise.resolve(list(since));
  });
  const shellMode = signal<ProjectTabBarMode>(options.mode ?? 'project');
  const controller = createStatsGlanceController({
    projects: signal<Project[]>(store.checkouts.map((id) => project(id, store.name))),
    shellMode,
    statsProjectId: signal<string | undefined>(undefined),
    liveStates,
    analyticsByProject: signal<Record<string, StatsDashboardState>>({}),
    terminalGroups: signal<TerminalDashboardGroup[]>([]),
    liveHistory,
    standbyEnteredAtMs: () => options.standbyAt,
    now: () => now,
    setInterval: () => 1,
    clearInterval: () => undefined,
  });
  return {
    controller,
    shellMode,
    liveHistory,
    reads,
    store,
    liveStates,
    advance(ms: number) {
      now += ms;
    },
  };
}

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('HS2-9AN4DN stats overnight timeline', () => {
  it('reads the history once when Stats opens, never while hidden or on a timer', async () => {
    const test = setup();
    await flush();
    expect(test.liveHistory).not.toHaveBeenCalled();
    test.shellMode.value = 'stats';
    await flush();
    expect(test.reads).toEqual([NOW - 12 * HOUR]);
    test.advance(HOUR);
    await flush();
    expect(test.liveHistory).toHaveBeenCalledTimes(1);
    const card = test.controller.overnight()!;
    expect(card).toMatchObject({ window: '12h', standbyAvailable: false, status: 'ready' });
    expect(card.lanes.map((lane) => lane.store)).toEqual([test.store.store]);
    // The pushed state extends the read segments to now, and the tick is placed in the window.
    expect(card.lanes[0].segments.at(-1)!.end_ms).toBe(NOW + HOUR);
    expect(card.lanes[0].ticks).toHaveLength(1);
  });

  it('defaults to since standby once it was entered, and a window change reads again', async () => {
    const test = setup({ mode: 'stats', standbyAt: NOW - 5 * HOUR });
    await flush();
    expect(test.reads).toEqual([NOW - 5 * HOUR]);
    expect(test.controller.overnight()).toMatchObject({ window: 'standby', standbyAvailable: true });
    test.controller.setOvernightWindow('24h');
    await flush();
    expect(test.reads).toEqual([NOW - 5 * HOUR, NOW - 24 * HOUR]);
    expect(test.controller.overnight()!.fromMs).toBe(NOW - 24 * HOUR);
    // Choosing the same window again reads nothing.
    test.controller.setOvernightWindow('24h');
    await flush();
    expect(test.reads).toHaveLength(2);
    // A resync re-reads once.
    await test.controller.refreshOvernight();
    expect(test.reads).toHaveLength(3);
  });

  it('keeps the read failure visible', async () => {
    const test = setup({ mode: 'stats', fail: true });
    await flush();
    expect(test.controller.overnight()).toMatchObject({ status: 'error', message: 'offline', lanes: [] });
  });

  it('is absent without a history transport', () => {
    const controller = createStatsGlanceController({
      projects: signal<Project[]>([]),
      shellMode: signal<ProjectTabBarMode>('project'),
      statsProjectId: signal<string | undefined>(undefined),
      liveStates: createLiveStateStore(),
      analyticsByProject: signal<Record<string, StatsDashboardState>>({}),
      terminalGroups: signal<TerminalDashboardGroup[]>([]),
      setInterval: () => 1,
      clearInterval: () => undefined,
    });
    expect(controller.overnight()).toBeUndefined();
  });
});
