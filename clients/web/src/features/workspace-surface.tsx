import type { Signal } from 'kerfjs';
import type { SafeHtml } from 'kerfjs/jsx-runtime';

import { type CheckoutTicketCounts, type CorruptTicket, type TicketRow as WireTicketRow } from '../api';
import { boardColumnHasMore, type BoardColumnPage, boardColumnStatus, isPerColumnBoardView } from '../board-pagination';
import type { CorruptTicketRecoveryState } from '../components/corrupt-ticket-row';
import { type NotificationView, notificationViewTitle } from '../components/notification-navigation';
import { type TicketEmptyStateProps } from '../components/ticket-empty-state';
import { TicketPageMore } from '../components/ticket-page-more';
import { type TicketRowProps } from '../components/ticket-row';
import { type WorkspaceSurfaceProps } from '../components/workspace-composition-surfaces';
import { type WorkspaceViewMode } from '../components/workspace-header';
import { type InlineSearchToken, orderedSearchText } from '../inline-search';
import { type Project } from '../interactions/types';
import type { PermissionHistoryItem, PermissionItem } from '../permission-notifications';
import { BOARD_COLUMN_PAGE_SIZE } from '../project-ticket-refresh';
import { ticketBoardGroups, ticketBoardGroupTotal } from '../ticket-board-layout';
import { type TicketScope, type TicketView } from '../ticket-views';
import type { WorkerTicketDetails, WorkerView } from '../worker-view-model';

/** Live application bindings createWorkspaceSurface reads (HS2-K0VC2C). */
export interface WorkspaceSurfaceDependencies {
  boardColumnLoading: Signal<Record<string, boolean>>;
  boardColumnPages: Signal<Record<string, BoardColumnPage>>;
  continueProgressiveTicketRendering: (total: number) => void;
  corruptRecovery: Signal<Record<string, CorruptTicketRecoveryState>>;
  corruptTickets: Signal<CorruptTicket[]>;
  error: Signal<string>;
  hideVerifiedColumn: () => boolean;
  loading: Signal<boolean>;
  notificationView: Signal<NotificationView>;
  partialSourcesByProject: Signal<Record<string, boolean>>;
  project: () => Project | undefined;
  projectPendingPermissions: (projectId?: string) => PermissionItem[];
  projectPermissionHistory: (projectId?: string) => PermissionHistoryItem[];
  renderedTicketLimit: Signal<number>;
  row: (ticket: WireTicketRow) => TicketRowProps;
  searchMatchKeys: Signal<Set<string> | undefined>;
  searchQuery: Signal<string>;
  searchTokens: Signal<InlineSearchToken[]>;
  selectedCorruptKey: Signal<string | undefined>;
  selectedProjectId: Signal<string>;
  selectedView: Signal<TicketView>;
  settingsWorkspace: (current: Project) => SafeHtml;
  ticketCollectionState: Signal<{ projectId: string; view: TicketView; status: 'loading' | 'error' } | undefined>;
  ticketCountsByProject: Signal<Record<string, CheckoutTicketCounts>>;
  ticketNextCursor: Signal<string | undefined>;
  ticketPageLoading: Signal<boolean>;
  tickets: Signal<WireTicketRow[]>;
  ticketViewTitle: (view: TicketScope) => string;
  viewMode: Signal<WorkspaceViewMode>;
  viewportMobile: Signal<boolean>;
  visibleTickets: () => WireTicketRow[];
  workspaceSearchActive: () => boolean;
  workerViews: Signal<Record<string, WorkerView>>;
  workerDetails: Signal<WorkerTicketDetails>;
  workerErrors: Signal<Record<string, string>>;
  workerUnsupportedSources: Signal<string[]>;
  workerLoading: Signal<boolean>;
  selectedTicketQualifiedId: () => string | undefined;
  /** The search's scope: the selected view, or All Tickets (HS2-4J6764). */
  searchScope: () => TicketScope;
}

/**
 * Projects the main workspace pane: the ticket list/board, settings, notifications, and empty
 * states. Extracted from the application runtime without behavior changes (HS2-K0VC2C).
 */
export function createWorkspaceSurface(dependencies: WorkspaceSurfaceDependencies) {
  const {
    boardColumnLoading,
    boardColumnPages,
    continueProgressiveTicketRendering,
    corruptRecovery,
    corruptTickets,
    error,
    hideVerifiedColumn,
    loading,
    notificationView,
    partialSourcesByProject,
    project,
    projectPendingPermissions,
    projectPermissionHistory,
    renderedTicketLimit,
    row,
    searchMatchKeys,
    searchQuery,
    searchTokens,
    selectedCorruptKey,
    selectedProjectId,
    selectedView,
    settingsWorkspace,
    ticketCollectionState,
    ticketCountsByProject,
    ticketNextCursor,
    ticketPageLoading,
    tickets,
    ticketViewTitle,
    viewMode,
    viewportMobile,
    visibleTickets,
    workspaceSearchActive,
    workerViews,
    workerDetails,
    workerErrors,
    workerUnsupportedSources,
    workerLoading,
    selectedTicketQualifiedId,
    searchScope,
  } = dependencies;
  function workspaceEmptyState(): TicketEmptyStateProps | undefined {
    if (loading.value) return { kind: 'loading' };
    const query = orderedSearchText(searchQuery.value, searchTokens.value, () => true);
    if (query) return { kind: searchMatchKeys.value === undefined ? 'searching' : 'search', query };
    const collection = ticketCollectionState.value;
    if (collection?.projectId === selectedProjectId.value && collection.view === selectedView.value)
      return {
        kind: collection.status === 'loading' ? 'view-loading' : 'view-error',
        viewLabel: ticketViewTitle(selectedView.value),
      };
    if (tickets.value.length === 0 && corruptTickets.value.length === 0) return { kind: 'project' };
    return { kind: 'view', viewLabel: ticketViewTitle(searchScope()) };
  }
  function workspaceSurfaceProps(): WorkspaceSurfaceProps {
    const waitingForBoardPages =
        viewMode.value === 'board' &&
        isPerColumnBoardView(selectedView.value, workspaceSearchActive()) &&
        Object.keys(boardColumnPages.value).length === 0 &&
        tickets.value.length === 0,
      shown = waitingForBoardPages ? [] : visibleTickets(),
      current = project(),
      emptyState: TicketEmptyStateProps | undefined = loading.value
        ? { kind: 'loading' }
        : waitingForBoardPages
          ? { kind: error.value ? 'view-error' : 'view-loading', viewLabel: ticketViewTitle(selectedView.value) }
          : workspaceEmptyState(),
      collectionLoading = emptyState?.kind === 'view-loading',
      hasMore = Boolean(ticketNextCursor.value) && !collectionLoading,
      more = hasMore ? <TicketPageMore loading={ticketPageLoading.value} /> : undefined;
    switch (viewMode.value) {
      case 'notifications': {
        const history = projectPermissionHistory(),
          cutoff = Date.now() - 24 * 60 * 60 * 1000,
          view = notificationView.value;
        return {
          kind: 'notifications',
          notifications: {
            title: notificationViewTitle(view),
            pending: view === 'pending' ? projectPendingPermissions() : [],
            history:
              view === 'day' ? history.filter((item) => item.resolvedAt >= cutoff) : view === 'week' ? history : [],
          },
        };
      }
      case 'settings':
        if (current) return { kind: 'settings', content: settingsWorkspace(current) };
        break;
      case 'workers':
        return {
          kind: 'workers',
          workers: {
            views: Object.values(workerViews.value),
            details: workerDetails.value,
            ticketRow: row,
            selectedQualifiedId: selectedTicketQualifiedId(),
            loading: workerLoading.value,
            sourceErrors: Object.values(workerErrors.value),
            unsupportedSources: workerUnsupportedSources.value,
            layout: viewportMobile.value ? 'paged' : 'grid',
          },
        };
      case 'board':
      case 'list':
        break;
      default:
        return viewMode.value satisfies never;
    }
    if (selectedView.value === 'errors')
      return {
        kind: 'errors',
        list: {
          tickets: [],
          corruptTickets: corruptTickets.value,
          corruptRecovery: corruptRecovery.value,
          selectedCorruptKey: selectedCorruptKey.value,
          label: 'Tickets with parsing errors',
          emptyState,
        },
      };
    continueProgressiveTicketRendering(shown.length);
    if (viewMode.value === 'board') {
      const counts =
          workspaceSearchActive() || collectionLoading
            ? undefined
            : ticketCountsByProject.value[selectedProjectId.value],
        hideVerified = hideVerifiedColumn(),
        scope = searchScope(),
        groups = ticketBoardGroups(shown, scope, hideVerified);
      // Per-column pagination (HS2-8NBGBX): each status column gets its own continuation from its own
      // cursor/loaded count, so a short column can load more independently of a long one. Single-column
      // views (backlog/archive/trash) and search keep the global cursor pinned to their one column.
      const perColumn = isPerColumnBoardView(scope, workspaceSearchActive()),
        lastShown = shown.at(-1),
        globalColumnId = perColumn
          ? undefined
          : (groups.find((group) => group.tickets.some((ticket) => ticket.id === lastShown?.id))?.id ??
            [...groups].reverse().find((group) => group.tickets.length)?.id ??
            groups[0]?.id);
      const columns = groups.map((group) => {
        const total = ticketBoardGroupTotal(group.id, group.tickets.length, scope, counts, hideVerified),
          status = boardColumnStatus(group.id),
          visible = Object.hasOwn(boardColumnPages.value, group.id)
            ? (boardColumnPages.value[group.id].visible ?? BOARD_COLUMN_PAGE_SIZE)
            : BOARD_COLUMN_PAGE_SIZE,
          hasCachedRows = group.tickets.length > visible;
        const continuation =
          perColumn && status
            ? hasCachedRows ||
              (counts && boardColumnHasMore(total, group.tickets.length, boardColumnPages.value[group.id]))
              ? { loading: boardColumnLoading.value[group.id] }
              : undefined
            : !collectionLoading && hasMore && globalColumnId === group.id
              ? { loading: ticketPageLoading.value }
              : undefined;
        return {
          ...group,
          totalCount: total,
          countPartial: partialSourcesByProject.value[selectedProjectId.value] ?? false,
          tickets: group.tickets.slice(0, Math.min(renderedTicketLimit.value, perColumn ? visible : Infinity)).map(row),
          continuation,
        };
      });
      // Phones page through one snapped column at a time instead of shrinking the grid (HS2-ZYJMDP).
      const layout = viewportMobile.value ? 'paged' : 'grid';
      return { kind: 'board', board: { columns, label: 'Project board', emptyState, layout } };
    }
    return {
      kind: 'list',
      list: {
        tickets: shown.slice(0, renderedTicketLimit.value).map(row),
        totalCount: shown.length,
        label: 'Project Tickets',
        emptyState,
      },
      more,
    };
  }
  return { workspaceEmptyState, workspaceSurfaceProps };
}
