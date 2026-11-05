import { wirePagedViews } from '@kerfjs/ui/wire-paged-views';
import { type Signal, signal } from 'kerfjs';

import { type CheckoutTicketCounts, type LiveStoreState, type TicketRow as WireTicketRow } from '../api';
import {
  GRID_SIDEBAR_PAGE_STORAGE_KEY,
  GRID_SIDEBAR_PAGED_VIEW_ID,
  type GridSidebarPage,
  parseGridSidebarPage,
} from '../components/terminal-operations-sidebar';
import { type TerminalOperationsSurfaceProps } from '../components/workspace-composition-surfaces';
import { gridSidebarLive } from '../grid-sidebar-live';
import { type Project } from '../interactions/types';
import { type LiveStateStore } from '../live-state';
import { ticketCompletionTrend } from '../ticket-completion-trend';

/** Live application bindings createTerminalOperationsSurface reads (HS2-K0VC2C). */
export interface TerminalOperationsSurfaceDependencies {
  partialSourcesByProject: Signal<Record<string, boolean>>;
  projects: Signal<Project[]>;
  projectTicketCounts: (projectId: string) => CheckoutTicketCounts;
  selectedProjectId: Signal<string>;
  ticketRowsByProject: Signal<Record<string, WireTicketRow[]>>;
  tickets: Signal<WireTicketRow[]>;
  /** The pushed per-store live state for the gauge and triage pages (HS2-JTTQMY, HS2-EZSQDF). */
  liveStates?: LiveStateStore;
  /** Per-viewer storage for the grid sidebar's chosen page (HS2-EZSQDF); `localStorage` in the app. */
  storage?: Pick<Storage, 'getItem' | 'setItem'>;
}

/** Re-render cadence for decaying rates: whole minutes, matching the token rate's window. */
const GLANCE_CLOCK_MS = 60_000;

/**
 * Projects the terminal operations dashboard's per-project completion and progress summary, and
 * (HS2-EZSQDF) the live gauge and triage numbers from the pushed live state. Rates decay with time,
 * so a local minute clock re-renders them; it never issues a request. Extracted from the
 * application runtime (HS2-K0VC2C).
 */
export function createTerminalOperationsSurface(dependencies: TerminalOperationsSurfaceDependencies) {
  const {
    partialSourcesByProject,
    projects,
    projectTicketCounts,
    selectedProjectId,
    ticketRowsByProject,
    tickets,
    liveStates,
    storage,
  } = dependencies;
  /** The grid sidebar's page: the viewer's last choice, Completion by default (HS2-EZSQDF). */
  const gridSidebarPage = signal<GridSidebarPage>(parseGridSidebarPage(readStoredPage(storage)));
  const glanceClock = signal(Date.now());
  let clockTimer: ReturnType<typeof setTimeout> | undefined;
  /** Tick at each minute boundary while a live store exists; a display timer only. */
  function scheduleGlanceClock() {
    if (clockTimer !== undefined || !liveStates) return;
    const now = Date.now();
    clockTimer = setTimeout(
      () => {
        clockTimer = undefined;
        glanceClock.value = Date.now();
        if (liveStates.states.peek().size > 0) scheduleGlanceClock();
      },
      GLANCE_CLOCK_MS - (now % GLANCE_CLOCK_MS),
    );
  }
  function terminalOperationsSurfaceProps(): TerminalOperationsSurfaceProps {
    const now = Math.max(glanceClock.value, Date.now());
    const hosted = new Map<string, LiveStoreState>();
    const summaries = projects.value.map((item) => {
      const rows = item.id === selectedProjectId.value ? tickets.value : (ticketRowsByProject.value[item.id] ?? []),
        counts = projectTicketCounts(item.id),
        trend = counts.completion_trend ?? ticketCompletionTrend(rows),
        stores = liveStates?.forProject(item.id) ?? [];
      for (const state of stores) hosted.set(state.store, state);
      const live = gridSidebarLive(stores, now);
      return {
        id: item.id,
        name: item.name,
        completedToday: counts.completed_today,
        inProgress: counts.started,
        trend,
        partial: partialSourcesByProject.value[item.id] ?? false,
        ...(live ? { live } : {}),
      };
    });
    if (hosted.size > 0) scheduleGlanceClock();
    // A store linked by several open projects counts once in All projects.
    const allLive = gridSidebarLive([...hosted.values()], now);
    return {
      projects: summaries,
      ...(allLive ? { allLive } : {}),
      page: gridSidebarPage.value,
    };
  }
  /** Record a person's page change: re-render on it and remember it for this viewer. */
  function selectGridSidebarPage(page: string) {
    const next = parseGridSidebarPage(page);
    gridSidebarPage.value = next;
    try {
      storage?.setItem(GRID_SIDEBAR_PAGE_STORAGE_KEY, next);
    } catch {
      // Storage can be unavailable (private mode, quota); the page still changes for this session.
    }
  }
  /**
   * Wire the grid sidebar's Kerf PagedView under `root` (swipe, trackpad scroll, dot click, arrow
   * keys); Kerf owns the mechanics and this surface owns the page. Returns a disposer.
   */
  function wire(root: HTMLElement): () => void {
    return wirePagedViews(root, {
      onPageChange: (page, view) => {
        if (view === GRID_SIDEBAR_PAGED_VIEW_ID) selectGridSidebarPage(page);
      },
    });
  }
  function dispose() {
    if (clockTimer !== undefined) clearTimeout(clockTimer);
    clockTimer = undefined;
  }
  return { terminalOperationsSurfaceProps, gridSidebarPage, selectGridSidebarPage, wire, dispose };
}

function readStoredPage(storage: Pick<Storage, 'getItem'> | undefined): string | null {
  try {
    return storage?.getItem(GRID_SIDEBAR_PAGE_STORAGE_KEY) ?? null;
  } catch {
    return null;
  }
}
