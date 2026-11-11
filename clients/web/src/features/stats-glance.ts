import { effect, type Signal, signal } from 'kerfjs';

import type { LiveHistoryList, LiveStoreState } from '../api';
import type { ProjectTabBarMode } from '../components/project-tab-bar';
import type { StatsDashboardState } from '../components/stats-dashboard';
import type { StatsOvernightProps } from '../components/stats-overnight';
import type { StatsRightNowProps } from '../components/stats-right-now';
import type { StatsThisMacProps } from '../components/stats-this-mac';
import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import type { HostHealthResourceState } from '../host-health-resource';
import type { Project } from '../interactions/types';
import type { LiveStateStore } from '../live-state';
import { extendSegments, hourTicks, overnightFrom, type OvernightWindow, timelineLane } from '../overnight-timeline';
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
  /** The server host's health for the This Mac cards (HS2-NS0AYR). */
  hostHealth?: { state: Signal<HostHealthResourceState> };
  /**
   * Read the recorded live state history since a time (HS2-9AN4DN). It is read when Stats opens, when
   * its window changes, and on the Stats project's stream resync; pushed snapshots extend it between reads.
   */
  liveHistory?: (sinceMs: number) => Promise<LiveHistoryList>;
  /** When standby was last entered in this session, for the "Since standby" window. */
  standbyEnteredAtMs?: () => number | undefined;
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
  /** The Overnight card's chosen window; unset means since standby when it was entered, else 12 hours. */
  const overnightChoice = signal<OvernightWindow | undefined>(undefined);
  const overnight = signal<{
    status: 'loading' | 'ready' | 'error';
    list?: LiveHistoryList;
    readAtMs?: number;
    message?: string;
  }>({ status: 'loading' });
  let overnightRead = 0;

  function overnightWindow(): OvernightWindow {
    const chosen = overnightChoice.value;
    if (chosen === 'standby' && dependencies.standbyEnteredAtMs?.() === undefined) return '12h';
    return chosen ?? (dependencies.standbyEnteredAtMs?.() === undefined ? '12h' : 'standby');
  }

  /** Read the history once for the current window; an older read that lands late is dropped. */
  async function refreshOvernight(): Promise<void> {
    const read = dependencies.liveHistory;
    if (!read) return;
    const request = ++overnightRead,
      at = now(),
      from = overnightFrom(overnightWindow(), at, dependencies.standbyEnteredAtMs?.());
    overnight.value = { ...overnight.peek(), status: overnight.peek().list ? 'ready' : 'loading' };
    try {
      const list = await read(from);
      if (request === overnightRead) overnight.value = { status: 'ready', list, readAtMs: at };
    } catch (reason) {
      if (request === overnightRead)
        overnight.value = {
          ...overnight.peek(),
          status: 'error',
          message: reason instanceof Error ? reason.message : String(reason),
        };
    }
  }

  function setOvernightWindow(window: OvernightWindow) {
    if (window === overnightChoice.peek()) return;
    overnightChoice.value = window;
    void refreshOvernight();
  }

  // The store's signal changes only when a newer snapshot applies, so this marks each live update.
  const disposeApplied = effect(() => {
    if (liveStates.states.value.size > 0) appliedAt.value = now();
  });
  const disposeClock = effect(() => {
    const visible = shellMode.value === 'stats';
    if (visible && timer === undefined) {
      clock.value = now();
      // Opening Stats reads the recorded history once (HS2-9AN4DN); nothing re-reads on a timer.
      queueMicrotask(() => {
        void refreshOvernight();
      });
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
    // All projects means the open project tabs, never every store the server hosts (HS2-JGW031).
    return projectId ? liveStates.forProject(projectId) : liveStates.forProjects(projects.value.map((item) => item.id));
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

  /** The Overnight card's input (HS2-9AN4DN): the read history, extended live, over the window. */
  function overnightCard(): StatsOvernightProps | undefined {
    if (!dependencies.liveHistory) return undefined;
    const nowMs = Math.max(clock.value, now()),
      window = overnightWindow(),
      from = overnightFrom(window, nowMs, dependencies.standbyEnteredAtMs?.()),
      read = overnight.value,
      states = scopedStates(),
      inScope = new Set(states.map((state) => state.store)),
      current = new Map(states.map((state) => [state.store, state]));
    const lanes = (read.list?.stores ?? [])
      .filter((store) => inScope.has(store.store))
      .sort((a, b) => a.name.localeCompare(b.name))
      .map((store) =>
        timelineLane(
          store,
          extendSegments(store.segments, current.get(store.store), read.readAtMs ?? nowMs, nowMs),
          from,
          nowMs,
        ),
      );
    return {
      window,
      standbyAvailable: dependencies.standbyEnteredAtMs?.() !== undefined,
      fromMs: from,
      toMs: nowMs,
      lanes,
      axis: hourTicks(from, nowMs),
      status: read.status,
      ...(read.message === undefined ? {} : { message: read.message }),
    };
  }

  /** The This Mac cards' input: the pushed server-host snapshot (HS2-NS0AYR). */
  function thisMac(): StatsThisMacProps | undefined {
    return dependencies.hostHealth ? { state: dependencies.hostHealth.state.value } : undefined;
  }

  function dispose() {
    disposeApplied();
    disposeClock();
    if (timer !== undefined) stopTimer(timer);
    timer = undefined;
  }

  return {
    rightNow,
    thisMac,
    overnight: overnightCard,
    setOvernightWindow,
    refreshOvernight,
    clock,
    appliedAt,
    dispose,
  };
}

export type StatsGlanceController = ReturnType<typeof createStatsGlanceController>;
