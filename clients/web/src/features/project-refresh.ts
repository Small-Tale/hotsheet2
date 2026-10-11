import type { Signal } from 'kerfjs';

import {
  Api,
  type CheckoutTicketCounts,
  type CheckoutTicketQuery,
  type CommandDefinition,
  type CommandRun,
  type CorruptTicket,
  type FullTicket,
  type RepositoryStatus,
  type TicketRow as WireTicketRow,
} from '../api';
import type { BoardColumnPage } from '../board-pagination';
import { corruptTicketKey } from '../components/corrupt-ticket-row';
import type { Project } from '../interactions/types';
import type { ProjectTicketSources } from '../new-ticket-source';
import { mergeRetainedCreatedRows, type PendingCreatedTickets } from '../pending-created-tickets';
import { type BoardRefreshSpec, loadProjectTicketRefresh } from '../project-ticket-refresh';
import type { RefreshBarrier } from '../refresh-barrier';
import { type TicketView, ticketViewQuery } from '../ticket-views';

/** The per-project projection a warm tab paints from before it revalidates (HS2-AZZ9TF). */
export interface CachedProjectProjection {
  corruptTickets: CorruptTicket[];
  repository: RepositoryStatus | null;
  repositoryError: string;
  commandDefinitions: CommandDefinition[];
  commandRuns: CommandRun[];
  searchMatchKeys?: Set<string>;
}

/** Options for one refresh of the selected project's tickets. */
export interface ProjectRefreshOptions {
  showLoading?: boolean;
  quiet?: boolean;
  refreshRepository?: boolean;
  onAvailable?: () => void;
}

/** Live application bindings the selected-project refresh owner reads and writes (HS2-BSSWGV). */
export interface ProjectRefreshDependencies {
  loading: Signal<boolean>;
  error: Signal<string>;
  selectedView: Signal<TicketView>;
  tickets: Signal<WireTicketRow[]>;
  ticketRowsByProject: Signal<Record<string, WireTicketRow[]>>;
  ticketNextCursor: Signal<string | undefined>;
  ticketCountsByProject: Signal<Record<string, CheckoutTicketCounts>>;
  partialSourcesByProject: Signal<Record<string, boolean>>;
  boardColumnPages: Signal<Record<string, BoardColumnPage>>;
  defaultProviders: Signal<Record<string, ProjectTicketSources | undefined>>;
  selectedTicketSlugs: Signal<string[]>;
  selectedTicket: Signal<FullTicket | null>;
  corruptTickets: Signal<CorruptTicket[]>;
  selectedCorruptKey: Signal<string | undefined>;
  repository: Signal<RepositoryStatus | null>;
  repositoryError: Signal<string>;
  commandDefinitions: Signal<CommandDefinition[]>;
  commandRuns: Signal<CommandRun[]>;
  projectProjectionById: Signal<Record<string, CachedProjectProjection>>;
  localTicketMutationBarrier: RefreshBarrier;
  pendingCreatedTickets: PendingCreatedTickets;
  /** The runtime's shift-click range anchor, cleared when its ticket leaves the refreshed rows. */
  ticketSelectionAnchor: { get: () => string | undefined; set: (value: string | undefined) => void };
  project: () => Project | undefined;
  sortedTicketQuery: (query: CheckoutTicketQuery) => CheckoutTicketQuery;
  boardRefreshSpec: (
    view: TicketView,
    projectId: string,
    loaded: { rows: readonly WireTicketRow[]; pages: Record<string, BoardColumnPage> },
  ) => BoardRefreshSpec | undefined;
  boardPagesWithVisibleLimit: (pages: Record<string, BoardColumnPage>) => Record<string, BoardColumnPage>;
  workspaceSearchActive: () => boolean;
  refreshTicketSearch: () => Promise<void>;
  markProjectWarm: (projectId: string) => void;
  recordAuthoritativeTicketTrend: (projectId: string, counts: CheckoutTicketCounts) => void;
  scheduleClaimLeaseExpiry: () => void;
  reconcileRefreshedSelected: (previous: FullTicket, refreshed: FullTicket) => void;
}

/**
 * Owns refreshing the selected project's tickets: the stale-response generation guard, the local
 * git-only first paint while a mixed-source merge waits, the authoritative merge into rows, counts,
 * board pages, selection, corrupt tickets, repository status, and the warm projection, and the
 * selected ticket's reconcile. Extracted from the application runtime without behavior changes
 * (HS2-BSSWGV).
 */
export function createProjectRefresh(dependencies: ProjectRefreshDependencies) {
  const {
    loading,
    error,
    selectedView,
    tickets,
    ticketRowsByProject,
    ticketNextCursor,
    ticketCountsByProject,
    partialSourcesByProject,
    boardColumnPages,
    defaultProviders,
    selectedTicketSlugs,
    selectedTicket,
    corruptTickets,
    selectedCorruptKey,
    repository,
    repositoryError,
    commandDefinitions,
    commandRuns,
    projectProjectionById,
    localTicketMutationBarrier,
    pendingCreatedTickets,
    project,
    sortedTicketQuery,
    boardRefreshSpec,
    boardPagesWithVisibleLimit,
    workspaceSearchActive,
    refreshTicketSearch,
    markProjectWarm,
    recordAuthoritativeTicketTrend,
    scheduleClaimLeaseExpiry,
    reconcileRefreshedSelected,
  } = dependencies;
  let projectRefreshGeneration = 0;
  async function refreshProject({
    showLoading = true,
    quiet = false,
    refreshRepository = false,
    onAvailable,
  }: ProjectRefreshOptions = {}) {
    const current = project(),
      generation = ++projectRefreshGeneration;
    if (!current) return;
    if (showLoading) loading.value = true;
    const active = () => generation === projectRefreshGeneration && project()?.id === current.id;
    try {
      // A read racing an in-flight save can answer with pre-save values; reconciled into the open editor after
      // that save committed, the user's own older text would look like someone else's edit and raise a merge
      // prompt. Direct refreshes (after an attachment upload, a restore, …) wait for local writes like
      // event-driven ones do; a write that begins later bumps the generation and drops this refresh (HS2-RE1PS6).
      await localTicketMutationBarrier.wait();
      if (!active()) return;
      const client = new Api(current.apiPath, '', { trackBusy: !quiet }),
        view = selectedView.value,
        query = sortedTicketQuery(ticketViewQuery(view)),
        board = boardRefreshSpec(view, current.id, { rows: tickets.value, pages: boardColumnPages.value });
      let fullSettled = false;
      const fullRefresh = loadProjectTicketRefresh(client, current.id, query, board);
      void fullRefresh.then(
        () => {
          fullSettled = true;
        },
        () => {
          fullSettled = true;
        },
      );
      // The complete merge can wait on an OS keychain prompt. Let the local git
      // source make the workspace useful first, then replace this partial view with
      // the authoritative mixed-source result when access is granted.
      const hasExternalSource =
        defaultProviders.value[current.id]?.sources.some((source) => source.provider !== 'git') ?? true;
      if (showLoading && hasExternalSource && !Object.hasOwn(ticketRowsByProject.value, current.id)) {
        void loadProjectTicketRefresh(client, current.id, { ...query, source: 'git' }, board)
          .then((local) => {
            if (!active() || fullSettled) return;
            if (!local.tickets || selectedView.value !== view || workspaceSearchActive()) {
              onAvailable?.();
              return;
            }
            markProjectWarm(current.id);
            const rows = mergeRetainedCreatedRows(
              local.tickets,
              pendingCreatedTickets.retain(current.id, local.tickets),
            );
            tickets.value = rows;
            ticketRowsByProject.value = { ...ticketRowsByProject.value, [current.id]: rows };
            ticketNextCursor.value = undefined;
            boardColumnPages.value = {};
            if (local.ticketCounts)
              ticketCountsByProject.value = { ...ticketCountsByProject.value, [current.id]: local.ticketCounts };
            partialSourcesByProject.value = { ...partialSourcesByProject.value, [current.id]: true };
            loading.value = false;
            onAvailable?.();
          })
          .catch(() => {
            /* The complete refresh still reports the authoritative error. */
          });
      }
      const [index, repositoryResult] = await Promise.all([
        fullRefresh,
        refreshRepository
          ? client
              .repositoryStatus(current.id)
              .then((status) => ({ status, error: '' }))
              .catch((reason: unknown) => ({
                status: null,
                error: reason instanceof Error ? reason.message : String(reason),
              }))
          : Promise.resolve(undefined),
      ]);
      if (!active()) return;
      markProjectWarm(current.id);
      if (index.tickets)
        partialSourcesByProject.value = {
          ...partialSourcesByProject.value,
          [current.id]: Boolean(index.sourceErrors?.length),
        };
      const mergedTickets = index.tickets
          ? mergeRetainedCreatedRows(index.tickets, pendingCreatedTickets.retain(current.id, index.tickets))
          : undefined,
        searchActive = workspaceSearchActive();
      if (index.tickets && mergedTickets) {
        if (index.ticketCounts) {
          ticketCountsByProject.value = { ...ticketCountsByProject.value, [current.id]: index.ticketCounts };
          recordAuthoritativeTicketTrend(current.id, index.ticketCounts);
        }
        // When a workspace search is active the visible rows are the search results, not this base-view page,
        // so replacing tickets/selection with the base page here would deselect a matching ticket that is not
        // on the first base page — and an autosave edit's own change event would keep re-triggering that,
        // yanking focus out of the field being edited (HS2-6AXG6Z). Leave the rows and selection to the
        // trailing refreshTicketSearch, which merges results and preserves selection.
        if (!searchActive && selectedView.value === view) {
          const finalTickets: WireTicketRow[] = mergedTickets;
          // Board columns load independently and restore any column the user had paged to its loaded length
          // in the same refresh, so the commit below is one assignment with no collapse-then-expand flash
          // (HS2-8NBGBX, HS2-HNZZHC). A list refresh leaves no per-column pages behind.
          boardColumnPages.value = boardPagesWithVisibleLimit(index.boardPages ?? {});
          tickets.value = finalTickets;
          ticketNextCursor.value = index.nextCursor;
          ticketRowsByProject.value = { ...ticketRowsByProject.value, [current.id]: finalTickets };
          const live = new Set(finalTickets.map((ticket) => ticket.slug)),
            selected = selectedTicketSlugs.value.filter((slug) => live.has(slug));
          if (selected.length !== selectedTicketSlugs.value.length) {
            selectedTicketSlugs.value = selected;
            const anchor = dependencies.ticketSelectionAnchor.get();
            if (anchor && !live.has(anchor)) dependencies.ticketSelectionAnchor.set(undefined);
            if (selected.length !== 1) selectedTicket.value = null;
          }
        }
        scheduleClaimLeaseExpiry();
      }
      corruptTickets.value = index.corruptTickets ?? [];
      if (corruptTickets.value.length === 0 && selectedView.value === 'errors') selectedView.value = 'all';
      if (
        selectedCorruptKey.value &&
        !corruptTickets.value.some((ticket) => corruptTicketKey(ticket) === selectedCorruptKey.value)
      )
        selectedCorruptKey.value = undefined;
      if (repositoryResult) {
        repository.value = repositoryResult.status;
        repositoryError.value = repositoryResult.error;
      }
      projectProjectionById.value = {
        ...projectProjectionById.value,
        [current.id]: {
          corruptTickets: corruptTickets.value,
          repository: repository.value,
          repositoryError: repositoryError.value,
          commandDefinitions: commandDefinitions.value,
          commandRuns: commandRuns.value,
        },
      };
      error.value = [index.ticketsError, index.corruptTicketsError].filter(Boolean).join(' · ');
      if (selectedTicket.value && mergedTickets) {
        const previous = selectedTicket.value,
          matching = mergedTickets.find((item) => item.id === previous.id);
        if (!matching && !searchActive) {
          selectedTicket.value = null;
          return;
        }
        // During an active search the selected ticket may legitimately be absent from the base-view page; keep
        // it selected and still reconcile its latest fields into the open inspector (HS2-6AXG6Z).
        if (matching || searchActive) {
          const refreshed = (await client.checkoutTicket(current.id, previous.qualified_id)).ticket;
          if (active() && selectedTicketSlugs.value.includes(refreshed.slug))
            reconcileRefreshedSelected(previous, refreshed);
        }
      }
      if (searchActive) void refreshTicketSearch();
    } catch (reason) {
      if (active()) error.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (showLoading && active()) loading.value = false;
    }
  }
  return {
    refreshProject,
    /** Drop any refresh already past the mutation barrier (a local write is about to apply). */
    invalidate() {
      projectRefreshGeneration += 1;
    },
  };
}
