import { signal } from 'kerfjs';
import { describe, expect, it, vi } from 'vitest';

import type { ProjectTabBarMode } from '../components/project-tab-bar';
import type { StatsDashboardState } from '../components/stats-dashboard';
import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import type { Project } from '../interactions/types';
import { createLiveStateStore } from '../live-state';
import { GLANCE_FIXTURE_NOW as NOW, glanceScenario } from '../ux-demo/live-state-fixtures';
import { createStatsGlanceController, STATS_GLANCE_TICK_MS } from './stats-glance';

const project = (id: string, name: string) => ({ id, name, apiPath: '/api' }) as unknown as Project;

function setup(mode: ProjectTabBarMode = 'stats') {
  let now = NOW;
  const timers = new Map<number, () => void>();
  let nextTimer = 1;
  const setIntervalMock = vi.fn((callback: () => void) => {
    timers.set(nextTimer, callback);
    return nextTimer++;
  });
  const clearIntervalMock = vi.fn((id: number) => timers.delete(id));
  const liveStates = createLiveStateStore();
  const projects = signal<Project[]>([
    project('hotsheet2-a1b2', 'Hot Sheet 2'),
    project('website-c3d4', 'Small Tale Website'),
    project('review-e5f6', 'UX Review'),
  ]);
  const shellMode = signal<ProjectTabBarMode>(mode);
  const statsProjectId = signal<string | undefined>(undefined);
  const analyticsByProject = signal<Record<string, StatsDashboardState>>({});
  const terminalGroups = signal<TerminalDashboardGroup[]>([
    {
      projectId: 'website-c3d4',
      sessions: [{ id: 't-plain' }, { id: 't-halted', halt: { error_type: 'overloaded', message: 'x', at: '' } }],
    } as unknown as TerminalDashboardGroup,
  ]);
  const controller = createStatsGlanceController({
    projects,
    shellMode,
    statsProjectId,
    liveStates,
    analyticsByProject,
    terminalGroups,
    now: () => now,
    setInterval: setIntervalMock,
    clearInterval: clearIntervalMock,
  });
  return {
    controller,
    liveStates,
    shellMode,
    statsProjectId,
    analyticsByProject,
    timers,
    setIntervalMock,
    clearIntervalMock,
    advance(ms: number) {
      now += ms;
      for (const callback of timers.values()) callback();
    },
  };
}

describe('HS2-ERTNJ7 stats glance controller', () => {
  it('runs its display clock only while Stats is shown and never as a request', () => {
    const test = setup('project');
    expect(test.setIntervalMock).not.toHaveBeenCalled();
    test.shellMode.value = 'stats';
    expect(test.setIntervalMock).toHaveBeenCalledWith(expect.any(Function), STATS_GLANCE_TICK_MS);
    test.shellMode.value = 'stats';
    expect(test.setIntervalMock).toHaveBeenCalledTimes(1);
    test.shellMode.value = 'terminals';
    expect(test.clearIntervalMock).toHaveBeenCalledTimes(1);
    expect(test.timers.size).toBe(0);
    test.shellMode.value = 'stats';
    expect(test.setIntervalMock).toHaveBeenCalledTimes(2);
    test.controller.dispose();
    expect(test.timers.size).toBe(0);
  });

  it('ages rows on the clock without a new snapshot', () => {
    const test = setup();
    for (const state of glanceScenario()) test.liveStates.apply(state);
    const before = test.controller.rightNow().rows.find((row) => row.name === 'Hot Sheet 2')!;
    test.advance(STATS_GLANCE_TICK_MS);
    const after = test.controller.rightNow().rows.find((row) => row.name === 'Hot Sheet 2')!;
    expect(after.progressAgeMs! - before.progressAgeMs!).toBe(STATS_GLANCE_TICK_MS);
    test.controller.dispose();
  });

  it('marks data as of the last applied snapshot', () => {
    const test = setup();
    expect(test.controller.rightNow().asOfMs).toBeUndefined();
    expect(test.controller.rightNow().stores).toBe(0);
    const [first] = glanceScenario();
    test.liveStates.apply(first);
    expect(test.controller.rightNow().asOfMs).toBe(NOW);
    test.advance(5_000);
    test.liveStates.apply(first); // not newer: unchanged
    expect(test.controller.rightNow().asOfMs).toBe(NOW);
    test.liveStates.apply({ ...first, revision: 2 });
    expect(test.controller.rightNow().asOfMs).toBe(NOW + 5_000);
    test.controller.dispose();
  });

  it('scopes to the Stats project (HS2-PZPD83) and routes actions to open projects and terminals', () => {
    const test = setup();
    for (const state of glanceScenario()) test.liveStates.apply(state);
    const all = test.controller.rightNow();
    expect(all.scope).toEqual({ kind: 'all' });
    expect(all.stores).toBe(4);
    expect(all.rows.find((row) => row.state === 'may_be_stuck')?.action).toEqual({
      kind: 'open-terminal',
      terminalKey: 'website-c3d4:t-halted',
    });
    // Kerf is not an open project: no action to route.
    expect(all.rows.find((row) => row.name === 'Kerf')?.action).toBeUndefined();
    test.statsProjectId.value = 'hotsheet2-a1b2';
    const scoped = test.controller.rightNow();
    expect(scoped.scope).toEqual({ kind: 'project', name: 'Hot Sheet 2' });
    expect(scoped.rows.map((row) => row.name)).toEqual(['Hot Sheet 2']);
    expect(scoped.tiles.needsYou).toBe(0);
    test.controller.dispose();
  });

  it('counts completions once per shared store, only after no scoped analytics report is loading', () => {
    const test = setup();
    const report = (stores: string[]) => ({
      status: 'ready' as const,
      report: {
        stores: stores.map((store_id) => ({
          store_id,
          ticket_flow: { throughput_by_day: { '2026-11-06': 3 } },
          usage: {},
        })),
        unavailable_sources: [],
      } as never,
    });
    test.analyticsByProject.value = { 'hotsheet2-a1b2': report(['s1']), 'website-c3d4': report(['s1', 's2']) };
    expect(test.controller.rightNow().completed).toBeUndefined();
    test.analyticsByProject.value = { ...test.analyticsByProject.value, 'review-e5f6': { status: 'loading' } };
    expect(test.controller.rightNow().completed).toBeUndefined();
    test.analyticsByProject.value = {
      ...test.analyticsByProject.value,
      'review-e5f6': { status: 'error', message: 'Unavailable' },
    };
    expect(test.controller.rightNow().completed).toEqual({ count: 6, sinceMs: Date.parse('2026-11-06T00:00:00Z') });
    test.statsProjectId.value = 'hotsheet2-a1b2';
    expect(test.controller.rightNow().completed?.count).toBe(3);
    test.controller.dispose();
  });
});
