import type { Signal } from 'kerfjs';

import { Api, type CheckoutTicketCounts, type CheckoutTicketQuery, type TicketRow as WireTicketRow } from '../api';
import {
  applyBoardColumnFetch,
  type BoardColumnPage,
  boardColumnStatus,
  boardColumnStatuses,
  isPerColumnBoardView,
  nextBoardColumnFetch,
} from '../board-pagination';
import type { WorkspaceViewMode } from '../components/workspace-header';
import type { Project } from '../interactions/types';
import { appendUniqueTicketRows, BOARD_COLUMN_PAGE_SIZE, type BoardRefreshSpec } from '../project-ticket-refresh';
import { ticketBoardGroups, ticketBoardGroupTotal } from '../ticket-board-layout';
import { type TicketView, ticketViewQuery } from '../ticket-views';

/** Live application bindings the board column paging owner reads and writes (HS2-H2R2JB). */
export interface BoardColumnPagingDependencies {
  viewMode: Signal<WorkspaceViewMode>;
  selectedProjectId: Signal<string>;
  selectedView: Signal<TicketView>;
  hideVerifiedByProject: Signal<Record<string, boolean>>;
  boardColumnPages: Signal<Record<string, BoardColumnPage>>;
  boardColumnLoading: Signal<Record<string, boolean>>;
  tickets: Signal<WireTicketRow[]>;
  ticketRowsByProject: Signal<Record<string, WireTicketRow[]>>;
  ticketCountsByProject: Signal<Record<string, CheckoutTicketCounts>>;
  partialSourcesByProject: Signal<Record<string, boolean>>;
  error: Signal<string>;
  project: () => Project | undefined;
  hideVerifiedColumn: () => boolean;
  workspaceSearchActive: () => boolean;
  sortedTicketQuery: (query: CheckoutTicketQuery) => CheckoutTicketQuery;
  visibleTickets: () => WireTicketRow[];
  refreshProject: (options: { showLoading: boolean }) => Promise<void>;
  loadNextTicketPage: () => Promise<void>;
  recordAuthoritativeTicketTrend: (projectId: string, counts: CheckoutTicketCounts) => void;
  scheduleClaimLeaseExpiry: () => void;
}

/**
 * Owns per-column board paging: the board refresh spec, bulk status-change refills, and one column's
 * Load more. The column page signals stay with the runtime, which also resets and caches them per
 * project. Extracted from the application runtime without behavior changes (HS2-H2R2JB).
 */
export function createBoardColumnPaging(dependencies: BoardColumnPagingDependencies) {
  const {
    viewMode,
    selectedProjectId,
    selectedView,
    hideVerifiedByProject,
    boardColumnPages,
    boardColumnLoading,
    tickets,
    ticketRowsByProject,
    ticketCountsByProject,
    partialSourcesByProject,
    error,
    project,
    hideVerifiedColumn,
    workspaceSearchActive,
    sortedTicketQuery,
    visibleTickets,
    refreshProject,
    loadNextTicketPage,
    recordAuthoritativeTicketTrend,
    scheduleClaimLeaseExpiry,
  } = dependencies;
  /**
   * The per-column board refresh for `view` in `projectId`, or undefined when that workspace is not a
   * per-column board (list mode, mobile, search, or a single-collection view). `loaded` is the board's
   * current rows and column pages: every column the board already paged asks for its loaded row count
   * back, so a refresh keeps an expanded column instead of collapsing it (HS2-HNZZHC).
   */
  function boardRefreshSpec(
    view: TicketView,
    projectId = selectedProjectId.value,
    loaded?: { rows: readonly WireTicketRow[]; pages: Record<string, BoardColumnPage> },
  ): BoardRefreshSpec | undefined {
    if (viewMode.value !== 'board') return undefined;
    if (!isPerColumnBoardView(view, projectId === selectedProjectId.value && workspaceSearchActive())) return undefined;
    const hideVerified = hideVerifiedByProject.value[projectId] ?? false,
      columns = ['not-started', 'started', 'completed', ...(hideVerified ? [] : ['verified'])].map((id) => ({
        id,
        statuses: boardColumnStatuses(id, hideVerified),
      })),
      wants: Record<string, number> = {};
    if (loaded)
      for (const column of columns)
        if (Object.hasOwn(loaded.pages, column.id))
          for (const status of column.statuses) wants[status] = countTicketsForStatus(loaded.rows, status);
    return { columns, wants };
  }
  function countTicketsForStatus(rows: readonly WireTicketRow[], status: string) {
    let total = 0;
    for (const ticket of rows) if ((ticket.status ?? 'not_started') === status) total += 1;
    return total;
  }
  function beginBulkBoardRefill(
    projectId: string,
    before: readonly WireTicketRow[],
    after: readonly WireTicketRow[],
    counts: CheckoutTicketCounts | undefined,
  ) {
    if (project()?.id !== projectId || viewMode.value !== 'board' || !counts) return;
    const changed = before.filter(
      (ticket) => after.find((item) => item.slug === ticket.slug)?.status !== ticket.status,
    );
    if (!changed.length) return;
    const hidden = hideVerifiedColumn(),
      groups = ticketBoardGroups(after, selectedView.value, hidden),
      pages = { ...boardColumnPages.value },
      loading = { ...boardColumnLoading.value };
    for (const group of groups) {
      if (
        !Object.hasOwn(pages, group.id) ||
        !changed.some((ticket) => boardColumnStatuses(group.id, hidden).includes(ticket.status ?? 'not_started'))
      )
        continue;
      const total = ticketBoardGroupTotal(group.id, group.tickets.length, selectedView.value, counts, hidden);
      if (total <= group.tickets.length) continue;
      Reflect.deleteProperty(pages, group.id);
      loading[group.id] = true;
    }
    boardColumnPages.value = pages;
    boardColumnLoading.value = loading;
  }
  async function finishBulkBoardRefill(projectId: string) {
    if (project()?.id !== projectId || !Object.values(boardColumnLoading.value).some(Boolean)) return;
    try {
      await refreshProject({ showLoading: false });
    } finally {
      if (project()?.id === projectId) boardColumnLoading.value = {};
    }
  }
  // Load the next status-filtered page for one board column (HS2-8NBGBX). Rows are appended to the flat
  // `tickets.value` union (deduped) so selection/inspector/mutations are unaffected; only this column's
  // cursor + loaded count advance, leaving the other columns' pagination untouched.
  async function loadBoardColumnMore(columnId: string) {
    if (!boardColumnStatus(columnId)) {
      void loadNextTicketPage();
      return;
    }
    const current = project(),
      view = selectedView.value;
    if (!current || boardColumnLoading.value[columnId]) return;
    const pageState = Object.hasOwn(boardColumnPages.value, columnId) ? boardColumnPages.value[columnId] : undefined,
      visible = pageState?.visible ?? BOARD_COLUMN_PAGE_SIZE,
      loaded =
        ticketBoardGroups(visibleTickets(), view, hideVerifiedColumn()).find((group) => group.id === columnId)?.tickets
          .length ?? 0;
    if (loaded > visible) {
      boardColumnPages.value = {
        ...boardColumnPages.value,
        [columnId]: { ...pageState, loaded: pageState?.loaded ?? loaded, visible: visible + BOARD_COLUMN_PAGE_SIZE },
      };
      return;
    }
    // A column pages its ordered statuses in turn (HS2-F2N4ZN): the merged Completed column exhausts
    // `completed`, then continues into `verified`, so verified rows beyond the initial global page stay
    // reachable through its own Load more. Single-status columns keep a one-entry status list.
    const statuses = boardColumnStatuses(columnId, hideVerifiedColumn());
    const target = nextBoardColumnFetch(statuses, boardColumnPages.value[columnId]);
    if (!target) return;
    boardColumnLoading.value = { ...boardColumnLoading.value, [columnId]: true };
    try {
      const query = sortedTicketQuery({ ...ticketViewQuery(view), status: target.status });
      const page = await new Api(current.apiPath).checkoutTicketPage(
        current.id,
        BOARD_COLUMN_PAGE_SIZE,
        target.cursor,
        query,
      );
      if (project()?.id !== current.id || selectedView.value !== view) return;
      const next = appendUniqueTicketRows(tickets.value, page.items);
      tickets.value = next;
      ticketRowsByProject.value = { ...ticketRowsByProject.value, [current.id]: next };
      ticketCountsByProject.value = { ...ticketCountsByProject.value, [current.id]: page.counts };
      recordAuthoritativeTicketTrend(current.id, page.counts);
      if (page.source_errors?.length) {
        partialSourcesByProject.value = { ...partialSourcesByProject.value, [current.id]: true };
        error.value = page.source_errors.join(' · ');
      }
      const loaded = statuses.reduce((sum, status) => sum + countTicketsForStatus(next, status), 0);
      boardColumnPages.value = {
        ...boardColumnPages.value,
        [columnId]: {
          ...applyBoardColumnFetch(statuses, boardColumnPages.value[columnId], target.status, page.next_cursor, loaded),
          visible: visible + BOARD_COLUMN_PAGE_SIZE,
        },
      };
      // Do not reset the global progressive-render cap here — that would collapse the already-rendered rows
      // in the other columns. The appended rows render within the current cap and grow via continueProgressiveTicketRendering.
      scheduleClaimLeaseExpiry();
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (project()?.id === current.id) boardColumnLoading.value = { ...boardColumnLoading.value, [columnId]: false };
    }
  }
  return { boardRefreshSpec, countTicketsForStatus, beginBulkBoardRefill, finishBulkBoardRefill, loadBoardColumnMore };
}
