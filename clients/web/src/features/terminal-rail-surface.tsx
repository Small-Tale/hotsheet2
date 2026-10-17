import type { SearchFieldModel } from '@kerfjs/ui/search-field-model';
import type { Signal } from 'kerfjs';

import {
  type Capabilities,
  type CheckoutTicketCounts,
  type CustomView,
  type FullTicket,
  type TicketRow as WireTicketRow,
} from '../api';
import { NotificationCenter } from '../components/notification-center';
import { TicketBoard } from '../components/ticket-board';
import { ticketInspectorPanel } from '../components/ticket-inspector';
import { TicketList } from '../components/ticket-list';
import { type TicketRowProps } from '../components/ticket-row';
import { type TerminalRailSurfaceProps } from '../components/workspace-composition-surfaces';
import type { TicketViewActionSpec } from '../components/workspace-controls';
import {
  WorkspaceControls,
  type WorkspaceSort,
  type WorkspaceSortDirection,
  workspaceUpNextState,
  type WorkspaceViewMode,
} from '../components/workspace-header';
import { effectiveSearch, type InlineSearchToken } from '../inline-search';
import { type Project } from '../interactions/types';
import type { PermissionHistoryItem, PermissionItem } from '../permission-notifications';
import { ticketBoardGroups } from '../ticket-board-layout';
import { canBulkUpdate } from '../ticket-bulk-operations';
import {
  canCreateTicketInView,
  customTicketViewId,
  type TicketScope,
  ticketsForView,
  type TicketView,
} from '../ticket-views';
import type { WorkspaceSortPreference } from '../workspace-preferences';
import { compareWorkspaceTickets } from '../workspace-ticket-sort';
import type { createInspectorSurface } from './inspector-surface';

/** Live application bindings createTerminalRailSurface reads (HS2-K0VC2C). */
export interface TerminalRailSurfaceDependencies {
  activeWorkspaceSort: () => WorkspaceSortPreference;
  capabilitiesFor: (connectionId: string) => Capabilities;
  customViewsFor: (projectId?: string) => CustomView[];
  hideVerifiedColumn: () => boolean;
  pendingPermissions: () => PermissionItem[];
  permissionHistory: () => PermissionHistoryItem[];
  project: () => Project | undefined;
  projects: Signal<Project[]>;
  projectTicketCounts: (projectId: string) => CheckoutTicketCounts;
  row: (ticket: WireTicketRow) => TicketRowProps;
  searchHelpOpen: Signal<boolean>;
  searchMatchKeys: Signal<Set<string> | undefined>;
  searchOpen: Signal<boolean>;
  searchQuery: Signal<string>;
  searchTokens: Signal<InlineSearchToken[]>;
  selectedInspectorProps: ReturnType<typeof createInspectorSurface>['selectedInspectorProps'];
  previewInspectorProps: ReturnType<typeof createInspectorSurface>['previewInspectorProps'];
  selectedRows: () => WireTicketRow[];
  selectedTicket: Signal<FullTicket | null>;
  selectedTicketSlugs: Signal<string[]>;
  selectedView: Signal<TicketView>;
  sort: { value: WorkspaceSort };
  sortDirection: { value: WorkspaceSortDirection };
  terminalRailScreen: Signal<'root' | 'ticket'>;
  tickets: Signal<WireTicketRow[]>;
  ticketSearchKey: (ticket: WireTicketRow) => string;
  ticketViewActionSpec: (
    view: TicketView,
    canCreate: boolean,
    label?: string,
    size?: 'default' | 'compact',
  ) => TicketViewActionSpec | undefined;
  ticketViewTitle: (view: TicketScope) => string;
  viewMode: Signal<WorkspaceViewMode>;
  workspaceSearchModel: SearchFieldModel;
}

/**
 * Chooses what the rail's pushed ticket detail renders: the loaded ticket, or (HS2-1ABEYX) the
 * selected row's partial detail while an uncached ticket's full detail loads.
 */
export function railDetailSource(
  screen: 'root' | 'ticket',
  selectedSlugs: readonly string[],
  loadedSlug: string | undefined,
  selectedRows: readonly WireTicketRow[],
): { kind: 'loaded' } | { kind: 'preview'; row: WireTicketRow } | undefined {
  if (screen !== 'ticket' || selectedSlugs.length !== 1) return undefined;
  const slug = selectedSlugs[0];
  if (loadedSlug === slug) return { kind: 'loaded' };
  const row = selectedRows.find((ticket) => ticket.slug === slug);
  return row ? { kind: 'preview', row } : undefined;
}

/**
 * Projects the terminal rail: its sessions, previews, conversation, and the docked ticket
 * inspector. Extracted from the application runtime without behavior changes (HS2-K0VC2C).
 */
export function createTerminalRailSurface(dependencies: TerminalRailSurfaceDependencies) {
  const {
    activeWorkspaceSort,
    capabilitiesFor,
    customViewsFor,
    hideVerifiedColumn,
    pendingPermissions,
    permissionHistory,
    project,
    projects,
    projectTicketCounts,
    row,
    searchHelpOpen,
    searchMatchKeys,
    searchOpen,
    searchQuery,
    searchTokens,
    selectedInspectorProps,
    previewInspectorProps,
    selectedRows,
    selectedTicket,
    selectedTicketSlugs,
    selectedView,
    sort,
    sortDirection,
    terminalRailScreen,
    tickets,
    ticketSearchKey,
    ticketViewActionSpec,
    ticketViewTitle,
    viewMode,
    workspaceSearchModel,
  } = dependencies;
  function terminalRailSurfaceProps(): TerminalRailSurfaceProps {
    const current = project(),
      mode = (() => {
        switch (viewMode.value) {
          case 'notifications':
            return 'notifications';
          case 'board':
            return 'board';
          case 'settings':
          case 'list':
            return 'list';
          default:
            return viewMode.value satisfies never;
        }
      })(),
      railView = selectedView.value === 'errors' ? 'all' : selectedView.value,
      selection = selectedRows(),
      canCreate = canCreateTicketInView(railView),
      effective = effectiveSearch(searchQuery.value, searchTokens.value),
      matches = searchMatchKeys.value;
    let shown = ticketsForView(tickets.value, railView);
    if (effective.text || effective.tokens.length)
      shown = matches ? shown.filter((ticket) => matches.has(ticketSearchKey(ticket))) : [];
    const activeSort = activeWorkspaceSort();
    shown = shown.slice().sort((a, b) => compareWorkspaceTickets(a, b, activeSort.sort, activeSort.sortDirection));
    const pending = current ? pendingPermissions().filter((item) => item.projectId === current.id) : [],
      history = current ? permissionHistory().filter((item) => item.projectId === current.id) : [];
    // The rail's columns view pages one snapped column at a time, like the phone board (HS2-656Q43);
    // it shows every filtered ticket, so columns carry no continuation.
    const content = (() => {
      switch (mode) {
        case 'notifications':
          return <NotificationCenter title="Notifications" pending={pending} history={history} inset="flush" />;
        case 'board':
          return (
            <TicketBoard
              columns={ticketBoardGroups(shown, railView, hideVerifiedColumn()).map((group) => ({
                ...group,
                totalCount: group.tickets.length,
                tickets: group.tickets.map(row),
              }))}
              label="Project board"
              layout="paged"
            />
          );
        case 'list':
          return <TicketList tickets={shown.map(row)} label="Project Tickets" />;
        default:
          return mode satisfies never;
      }
    })();
    // The pushed ticket detail is the rail NavStack's second view (HS2-FY06N4); its key follows
    // the ticket so a different ticket is a new push. An uncached ticket pushes its row-backed
    // partial detail while the full detail loads, under the same key (HS2-1ABEYX).
    const railSource = railDetailSource(
        terminalRailScreen.value,
        selectedTicketSlugs.value,
        selectedTicket.value?.slug,
        selection,
      ),
      railInspectorProps =
        railSource?.kind === 'loaded'
          ? selectedInspectorProps()
          : railSource
            ? previewInspectorProps(railSource.row)
            : undefined;
    return {
      rail: {
        projects: projects.value.map((item) => ({ id: item.id, name: item.name })),
        selectedProjectId: current?.id ?? '',
        views: [
          { id: 'all', label: 'Queue' },
          { id: 'backlog', label: 'Backlog' },
          { id: 'archive', label: 'Archive' },
          ...((current && (projectTicketCounts(current.id).trash ?? 0) > 0) || railView === 'trash'
            ? [{ id: 'trash', label: 'Trash' }]
            : []),
          ...customViewsFor(current?.id).map((view) => ({ id: customTicketViewId(view.id), label: view.name })),
        ],
        selectedViewId: railView,
        controls: (
          <WorkspaceControls
            mode={mode}
            presentation="rail"
            searchOpen={searchOpen.value}
            searchModel={workspaceSearchModel}
            searchHelpOpen={searchHelpOpen.value}
            sort={sort.value}
            sortDirection={sortDirection.value}
            notificationCount={pending.length}
            selectedTicketCount={selection.length}
            selectedTicketsUpNext={workspaceUpNextState(selection.map((ticket) => ticket.up_next))}
            selectedTicketsUpNextEligible={
              selection.length > 0 &&
              selection.every((ticket) => ticket.status === 'not_started' || ticket.status === 'started')
            }
            selectedTicketsMutable={canBulkUpdate(selection, capabilitiesFor)}
          />
        ),
        content,
        detail: railInspectorProps
          ? { key: `ticket:${railInspectorProps.slug}`, parts: ticketInspectorPanel(railInspectorProps) }
          : undefined,
        title: mode === 'notifications' ? 'Notifications' : ticketViewTitle(railView),
        action: mode === 'notifications' ? undefined : ticketViewActionSpec(railView, canCreate, 'Ticket…', 'compact'),
      },
    };
  }
  return { terminalRailSurfaceProps };
}
