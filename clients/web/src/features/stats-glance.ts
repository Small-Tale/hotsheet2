import { effect, type Signal, signal } from 'kerfjs';

import type { LiveStoreState } from '../api';
import type { ProjectTabBarMode } from '../components/project-tab-bar';
import type { StatsDashboardState } from '../components/stats-dashboard';
import type { StatsRightNowProps } from '../components/stats-right-now';
import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import type { Project } from '../interactions/types';
import type { LiveStateStore } from '../live-state';
import { completedSinceUtcMidnight, rightNowRows, rightNowTiles } from '../stats-right-now';

/** How often the Stats glance re-renders ages and decaying rates. A local display timer only. */
export const STATS_GLANCE_TICK_MS = 30_000;

export interface StatsGlanceDependencies {
  projects: Signal<Project[]>;
  shellMode: Signal<ProjectTabBarMode>;
  statsProjectId: Signal<string | undefined>;
  liveStates: LiveStateStore;
  analyticsByProject: Signal<Record<string, StatsDashboardState>>;
  terminalGroups: Signal<TerminalDashboardGroup[]>;
  /** Injected for tests. */
  now?: () => number;
  setInterval?: (callback: () => void, ms: number) => number;
  clearInterval?: (id: number) => void;
}

/**
 * The Stats page's live glance surfaces (HS2-ERTNJ7, docs/34-glance-surfaces.md §34.8). It derives
 * the Right now props from the pushed live state and owns the display clock that ages them. The
 * clock runs only while Stats is shown and never issues a request: snapshots arrive over the change
 * stream (HS2-JTTQMY).
 */
export function createStatsGlanceController(dependencies: StatsGlanceDependencies) {
  const { projects, shellMode, statsProjectId, liveStates, analyticsByProject, terminalGroups } = dependencies;
  const now = dependencies.now ?? (() => Date.now());
  const startTimer = dependencies.setInterval ?? ((callback, ms) => window.setInterval(callback, ms));
  const stopTimer =
    dependencies.clearInterval ??
    ((id) => {
      window.clearInterval(id);
    });
  const clock = signal(now());
  /** When a pushed snapshot last changed what the client holds. */
  const appliedAt = signal<number | undefined>(undefined);
  let timer: number | undefined;

  // The store's signal changes only when a newer snapshot applies, so this marks each live update.
  const disposeApplied = effect(() => {
    if (liveStates.states.value.size > 0) appliedAt.value = now();
  });
  const disposeClock = effect(() => {
    const visible = shellMode.value === 'stats';
    if (visible && timer === undefined) {
      clock.value = now();
      timer = startTimer(() => {
        clock.value = now();
      }, STATS_GLANCE_TICK_MS);
    } else if (!visible && timer !== undefined) {
      stopTimer(timer);
      timer = undefined;
    }
  });

  /** The stores in Stats' scope (HS2-PZPD83). */
  function scopedStates(): LiveStoreState[] {
    const projectId = statsProjectId.value;
    return projectId ? liveStates.forProject(projectId) : [...liveStates.states.value.values()];
  }

  function projectFor(state: LiveStoreState): string | undefined {
    const open = new Set(projects.value.map((item) => item.id));
    const scoped = statsProjectId.value;
    if (scoped && state.checkouts.includes(scoped) && open.has(scoped)) return scoped;
    return state.checkouts.find((id) => open.has(id));
  }

  function terminalFor(projectId: string): string | undefined {
    const sessions = terminalGroups.value.find((group) => group.projectId === projectId)?.sessions ?? [];
    const session = sessions.find((item) => item.halt) ?? sessions.at(0);
    return session ? `${projectId}:${session.id}` : undefined;
  }

  function rightNow(): StatsRightNowProps {
    const nowMs = Math.max(clock.value, now());
    const states = scopedStates();
    const projectId = statsProjectId.value;
    const project = projectId ? projects.value.find((item) => item.id === projectId) : undefined;
    const analytics: (StatsDashboardState | undefined)[] = projectId
      ? [analyticsByProject.value[projectId]]
      : projects.value.map((item) => analyticsByProject.value[item.id]);
    const reports = analytics.flatMap((state) => (state?.status === 'ready' ? [state.report] : []));
    // Completions show once nothing in scope is still loading; a failed report is excluded, as in
    // Ticket flow and usage, which names it in its own banner.
    const ready = reports.length > 0 && analytics.every((state) => state !== undefined && state.status !== 'loading');
    // Shared stores are counted once, as in Ticket flow and usage.
    const byDay: Record<string, number> = {};
    const counted = new Set<string>();
    for (const report of reports)
      for (const store of report.stores) {
        if (counted.has(store.store_id)) continue;
        counted.add(store.store_id);
        for (const [day, count] of Object.entries(store.ticket_flow.throughput_by_day))
          byDay[day] = (byDay[day] ?? 0) + count;
      }
    return {
      scope: project ? { kind: 'project', name: project.name } : { kind: 'all' },
      stores: states.length,
      tiles: rightNowTiles(states, nowMs),
      rows: rightNowRows(states, nowMs, { projectFor, terminalFor }),
      ...(ready ? { completed: completedSinceUtcMidnight(byDay, nowMs) } : {}),
      ...(appliedAt.value === undefined ? {} : { asOfMs: appliedAt.value }),
    };
  }

  function dispose() {
    disposeApplied();
    disposeClock();
    if (timer !== undefined) stopTimer(timer);
    timer = undefined;
  }

  return { rightNow, clock, appliedAt, dispose };
}

export type StatsGlanceController = ReturnType<typeof createStatsGlanceController>;
