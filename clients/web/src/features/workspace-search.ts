import { batch, effect, type Signal } from 'kerfjs';

import {
  collectMatchingSearchPages,
  filterAdvancedSearchResults,
  usesAdvancedSearchExpression,
  usesBooleanSearchExpression,
} from '../advanced-search';
import { Api, type CheckoutTicketQuery, type CustomView, type TicketRow as WireTicketRow } from '../api';
import { effectiveSearch, type InlineSearchToken, sameInlineSearchState, tokenQuery } from '../inline-search';
import { restoreInlineSearchCaret } from '../inline-search-caret';
import type { Project } from '../interactions/types';
import { customViewSearch } from '../saved-views';
import { createSearchProjection } from '../search-projection-coalescing';
import { createTicketSearchModel, inlineSearchTokens } from '../ticket-search-model';
import {
  ALL_TICKETS_SCOPE,
  canSearchAllTickets,
  customTicketViewKey,
  type TicketScope,
  ticketSearchCountViews,
  ticketSearchScope,
  type TicketView,
  ticketViewQuery,
} from '../ticket-views';
import type { WorkspaceSortPreference } from '../workspace-preferences';

/** Per-view sidebar match counts for the current search-bar query. */
export interface SidebarSearchCounts {
  projectId: string;
  signature: string;
  generation: number;
  values: Partial<Record<string, number>>;
  pending: string[];
}

/** Live application bindings the workspace search reads and writes (HS2-3JGWTV). */
export interface WorkspaceSearchDependencies {
  searchQuery: Signal<string>;
  searchTokens: Signal<InlineSearchToken[]>;
  searchHelpOpen: Signal<boolean>;
  searchMatchKeys: Signal<Set<string> | undefined>;
  sidebarSearchCounts: Signal<SidebarSearchCounts | undefined>;
  selectedView: Signal<TicketView>;
  /** The transient All Tickets search mode; it lasts only while the search bar holds a query (HS2-4J6764). */
  searchAllTickets: Signal<boolean>;
  error: Signal<string>;
  ticketPageQuery: Signal<CheckoutTicketQuery>;
  ticketNextCursor: Signal<string | undefined>;
  tickets: Signal<WireTicketRow[]>;
  ticketRowsByProject: Signal<Record<string, WireTicketRow[]>>;
  project: () => Project | undefined;
  customViewFor: (view: TicketView, projectId?: string) => CustomView | undefined;
  customViewsFor: (projectId?: string) => CustomView[];
  activeWorkspaceSort: () => WorkspaceSortPreference;
  ticketSearchKey: (ticket: WireTicketRow) => string;
  mergeTicketLinkRows: (existing: readonly WireTicketRow[], incoming: readonly WireTicketRow[]) => WireTicketRow[];
  refreshProject: (options: { showLoading: boolean }) => Promise<unknown>;
  resetBoardColumnPages: () => void;
  resetProgressiveTicketRendering: () => void;
  scheduleProjectSessionPersistence: () => void;
}

/**
 * Owns the workspace search: Kerf's token search model, the debounced server search, boolean
 * expression paging, the sidebar per-view match counts, and the partial-source warning.
 * Extracted from the application runtime without behavior changes (HS2-3JGWTV).
 */
export function createWorkspaceSearchController(dependencies: WorkspaceSearchDependencies) {
  const {
    searchQuery,
    searchTokens,
    searchHelpOpen,
    searchMatchKeys,
    sidebarSearchCounts,
    selectedView,
    searchAllTickets,
    error,
    ticketPageQuery,
    ticketNextCursor,
    tickets,
    ticketRowsByProject,
    project,
    customViewFor,
    customViewsFor,
    activeWorkspaceSort,
    ticketSearchKey,
    mergeTicketLinkRows,
    refreshProject,
    resetBoardColumnPages,
    resetProgressiveTicketRendering,
    scheduleProjectSessionPersistence,
  } = dependencies;
  /** Whether the search bar itself holds a query. */
  function searchBarActive() {
    return Boolean(searchQuery.value.trim() || searchTokens.value.length);
  }
  /** Whether the visible rows come from a search: a search-bar query or a selected shared view's query. */
  function workspaceSearchActive() {
    return searchBarActive() || customViewFor(selectedView.value) !== undefined;
  }
  /** The scope the search covers: All Tickets while that mode is on, else the selected view (HS2-4J6764). */
  function searchScope(view = selectedView.value): TicketScope {
    return ticketSearchScope(view, searchAllTickets.value, searchBarActive());
  }
  /** Whether the floating search toolbar offers All Tickets: a bar search outside Trash and Ticket errors. */
  function allTicketsSearchAvailable() {
    return searchBarActive() && canSearchAllTickets(selectedView.value);
  }
  /** Switch the active search into All Tickets (`true`) or back to the view it was started from. */
  function setAllTicketsSearch(allTickets: boolean) {
    if (!allTicketsSearchAvailable() || searchAllTickets.value === allTickets) return;
    searchAllTickets.value = allTickets;
    resetProgressiveTicketRendering();
    searchMatchKeys.value = undefined;
    void refreshTicketSearch();
  }
  /** The search that selects the visible rows: the bar query, scoped by the selected shared view's query. */
  function selectedViewSearch(view = selectedView.value) {
    const bar = effectiveSearch(searchQuery.value, searchTokens.value),
      definition = searchScope(view) === ALL_TICKETS_SCOPE ? undefined : customViewFor(view);
    return definition ? combinedCustomViewSearch(definition, bar) : bar;
  }

  let searchTimer: number | undefined,
    searchGeneration = 0,
    searchPartialWarning = '',
    searchReplacedError = '';
  function updateSearchPartialWarning(message: string) {
    if (message) {
      if (error.value && error.value !== searchPartialWarning) searchReplacedError = error.value;
      searchPartialWarning = message;
      error.value = message;
    } else if (error.value === searchPartialWarning) {
      error.value = searchReplacedError;
      searchPartialWarning = '';
      searchReplacedError = '';
    } else {
      searchPartialWarning = '';
      searchReplacedError = '';
    }
  }
  // Kerf's managed SearchFieldModel owns the workspace search text, its chips, and the in-place tag
  // completion (HS2-5JXBQY); `searchQuery`/`searchTokens` are projections of its state for the rest
  // of the app, and every change schedules the debounced ticket search.
  const workspaceSearchModel = createTicketSearchModel({
    tags: () => availableSearchTags(),
    onClear: () => {
      searchHelpOpen.value = false;
    },
  });
  // Plain typing reaches the projections at a typing pause, then searches at once; chips and
  // empty/non-empty changes publish immediately (HS2-X72PD6).
  const searchProjection = createSearchProjection(
    () => ({ query: searchQuery.peek(), tokens: searchTokens.peek() }),
    ({ query, tokens }, coalesced) => {
      const committed = tokens.length > searchTokens.peek().length;
      batch(() => {
        searchQuery.value = query;
        searchTokens.value = [...tokens];
        if (committed) searchHelpOpen.value = false;
      });
      scheduleTicketSearch(coalesced ? 0 : undefined);
    },
  );
  let searchEditorRevision = workspaceSearchModel.editorRevision.peek();
  effect(() => {
    const state = workspaceSearchModel.state.value,
      tokens = inlineSearchTokens(state),
      // A programmatic replacement (a restored session or saved view) is not typing.
      revision = workspaceSearchModel.editorRevision.value,
      replaced = revision !== searchEditorRevision;
    searchEditorRevision = revision;
    if (sameInlineSearchState(searchQuery.value, searchTokens.value, state.query, tokens)) {
      searchProjection.cancel();
      return;
    }
    searchProjection.update({ query: state.query, tokens }, { immediate: replaced });
    // Typing still waiting for its pause holds off a search armed by an earlier edit, as each
    // keystroke used to by rescheduling it.
    if (searchProjection.pending() && searchTimer !== undefined) {
      window.clearTimeout(searchTimer);
      searchTimer = undefined;
    }
  });
  /** The caret position a workspace chip occupies, for focus restoration around Kerf's chip actions. */
  function workspaceSearchTokenOffset(raw: string) {
    return workspaceSearchModel.state.value.tokens.find((token) => token.value === raw)?.offset;
  }
  type EffectiveTicketSearch = ReturnType<typeof effectiveSearch>;
  const searchSignature = () => JSON.stringify([searchQuery.value.trim(), searchTokens.value]);
  const searchScopeQuery = (view: TicketScope) => (customTicketViewKey(view) ? {} : ticketViewQuery(view));
  function sortedTicketQuery(query: CheckoutTicketQuery): CheckoutTicketQuery {
    const active = activeWorkspaceSort();
    return { ...query, sort: active.sort, direction: active.sortDirection };
  }
  function matchedSearchRows(rows: WireTicketRow[], effective: EffectiveTicketSearch) {
    const advanced = usesAdvancedSearchExpression(effective.text),
      matched = advanced ? filterAdvancedSearchResults(rows, effective.text) : rows,
      tags = effective.tokens
        .filter((token): token is Extract<InlineSearchToken, { kind: 'tag' }> => token.kind === 'tag')
        .map((token) => token.value.toLowerCase());
    return matched.filter((ticket) => tags.every((tag) => ticket.tags.some((value) => value.toLowerCase() === tag)));
  }
  function searchRequest(effective: EffectiveTicketSearch, view: TicketScope): CheckoutTicketQuery {
    const advanced = usesAdvancedSearchExpression(effective.text),
      serverTokens = usesBooleanSearchExpression(effective.text) ? [] : effective.tokens,
      tokenFilters = tokenQuery(serverTokens);
    // Only a lone positive review term can be pushed down. In a composed expression,
    // the provider must leave all candidates for the local Boolean evaluator.
    if (effective.text.trim().toLowerCase() !== 'is:needs-review') delete tokenFilters.needs_review;
    return sortedTicketQuery({
      ...searchScopeQuery(view),
      text: advanced ? '' : effective.text,
      ...tokenFilters,
    });
  }
  function activeSidebarSearchCount(state: SidebarSearchCounts) {
    const active = sidebarSearchCounts.value;
    return (
      active?.projectId === state.projectId &&
      active.signature === state.signature &&
      active.generation === state.generation
    );
  }
  function updateSidebarSearchCount(state: SidebarSearchCounts, view: TicketView, count?: number) {
    const active = sidebarSearchCounts.value;
    if (!activeSidebarSearchCount(state) || !active) return;
    const values =
      count === undefined
        ? Object.fromEntries(Object.entries(active.values).filter(([id]) => id !== view))
        : { ...active.values, [view]: count };
    sidebarSearchCounts.value = {
      ...active,
      values,
      pending: active.pending.filter((id) => id !== view),
    };
  }
  async function countSearchView(
    client: Api,
    current: Project,
    view: TicketView,
    effective: EffectiveTicketSearch,
    state: SidebarSearchCounts,
    first?: { rows: WireTicketRow[]; cursor?: string; query: CheckoutTicketQuery; sourceErrors?: string[] },
  ) {
    let count = first ? matchedSearchRows(first.rows, effective).length : 0,
      cursor = first?.cursor;
    const sourceErrors = new Set(first?.sourceErrors ?? []);
    const query = first?.query ?? searchRequest(effective, view);
    do {
      if (!first || cursor) {
        const page = await client.checkoutTicketPage(current.id, 500, cursor, query);
        count += matchedSearchRows(page.items, effective).length;
        page.source_errors?.forEach((message) => sourceErrors.add(message));
        cursor = page.next_cursor;
      } else cursor = undefined;
      first = undefined;
    } while (cursor && activeSidebarSearchCount(state));
    updateSidebarSearchCount(state, view, sourceErrors.size ? undefined : count);
    if (sourceErrors.size && activeSidebarSearchCount(state)) updateSearchPartialWarning([...sourceErrors].join(' · '));
  }
  function combinedCustomViewSearch(view: CustomView, effective: EffectiveTicketSearch) {
    return customViewSearch(view, effective.text, effective.tokens);
  }
  async function refreshSidebarSearchCounts(
    current: Project,
    selected: TicketScope,
    effective: EffectiveTicketSearch,
    state: SidebarSearchCounts,
    first: { rows: WireTicketRow[]; cursor?: string; query: CheckoutTicketQuery; sourceErrors?: string[] },
    refreshEveryView: boolean,
  ) {
    const client = new Api(current.apiPath);
    // An All Tickets page spans every view, so it seeds no single view's count; each is counted below.
    if (selected !== ALL_TICKETS_SCOPE) {
      const selectedDefinition = customViewFor(selected, current.id),
        selectedSearch = selectedDefinition ? combinedCustomViewSearch(selectedDefinition, effective) : effective;
      try {
        await countSearchView(client, current, selected, selectedSearch, state, first);
      } catch (reason) {
        updateSidebarSearchCount(state, selected);
        if (activeSidebarSearchCount(state))
          updateSearchPartialWarning(reason instanceof Error ? reason.message : String(reason));
      }
    }
    if (!refreshEveryView || !activeSidebarSearchCount(state)) return;
    const views = ticketSearchCountViews(
      customViewsFor(current.id).map((view) => view.id),
      selected,
    );
    await Promise.all(
      views
        .filter((view) => view !== selected)
        .map(async (view) => {
          const definition = customViewFor(view, current.id),
            viewSearch = definition ? combinedCustomViewSearch(definition, effective) : effective;
          try {
            await countSearchView(client, current, view, viewSearch, state);
          } catch (reason) {
            updateSidebarSearchCount(state, view);
            if (activeSidebarSearchCount(state))
              updateSearchPartialWarning(reason instanceof Error ? reason.message : String(reason));
          }
        }),
    );
  }
  async function refreshTicketSearch() {
    resetBoardColumnPages();
    const current = project(),
      view = selectedView.value,
      scope = searchScope(view),
      effective = selectedViewSearch(view),
      barEffective = effectiveSearch(searchQuery.value, searchTokens.value),
      countSidebar = Boolean(barEffective.text || barEffective.tokens.length),
      signature = searchSignature(),
      generation = ++searchGeneration;
    if (searchTimer !== undefined) {
      window.clearTimeout(searchTimer);
      searchTimer = undefined;
    }
    if (!current || (!effective.text && !effective.tokens.length)) {
      updateSearchPartialWarning('');
      // The search cursor belongs to the search query; drop both together until the base refresh pairs a
      // fresh cursor with its own query (HS2-38MPMC).
      ticketPageQuery.value = {};
      ticketNextCursor.value = undefined;
      searchMatchKeys.value = undefined;
      sidebarSearchCounts.value = undefined;
      if (current) void refreshProject({ showLoading: false });
      return;
    }
    const countViews = ticketSearchCountViews(
        customViewsFor(current.id).map((item) => item.id),
        scope,
      ),
      previous = sidebarSearchCounts.value,
      countsComplete =
        previous?.projectId === current.id &&
        previous.signature === signature &&
        previous.pending.length === 0 &&
        countViews.every((id) => Object.prototype.hasOwnProperty.call(previous.values, id)),
      countState: SidebarSearchCounts = {
        projectId: current.id,
        signature,
        generation,
        values: countsComplete ? { ...previous.values } : {},
        pending: countsComplete ? (scope === ALL_TICKETS_SCOPE ? [] : [view]) : countViews,
      };
    sidebarSearchCounts.value = countSidebar ? countState : undefined;
    try {
      const query = searchRequest(effective, scope),
        client = new Api(current.apiPath),
        page = await client.checkoutTicketPage(current.id, 200, undefined, query),
        boolean = usesBooleanSearchExpression(effective.text),
        collectAll = boolean || (usesAdvancedSearchExpression(effective.text) && !query.needs_review),
        active = () =>
          generation === searchGeneration &&
          project()?.id === current.id &&
          selectedView.value === view &&
          searchScope() === scope &&
          searchSignature() === signature,
        sourceErrors = new Set(page.source_errors ?? []),
        allMatches = collectAll
          ? await collectMatchingSearchPages(
              page,
              async (cursor) => {
                const next = await client.checkoutTicketPage(current.id, 500, cursor, query);
                next.source_errors?.forEach((message) => sourceErrors.add(message));
                return next;
              },
              (row) => matchedSearchRows([row], effective).length === 1,
              active,
            )
          : undefined;
      if (collectAll && !allMatches) return;
      const rows = collectAll ? allMatches! : page.items,
        matched = collectAll ? rows : matchedSearchRows(rows, effective);
      if (!active()) return;
      ticketPageQuery.value = query;
      ticketNextCursor.value = collectAll ? undefined : page.next_cursor;
      tickets.value = mergeTicketLinkRows(tickets.value, rows);
      ticketRowsByProject.value = { ...ticketRowsByProject.value, [current.id]: tickets.value };
      searchMatchKeys.value = new Set(matched.map(ticketSearchKey));
      updateSearchPartialWarning([...sourceErrors].join(' · '));
      // Sidebar counts report the search-bar query per view, so a shared view alone shows ordinary counts.
      if (countSidebar)
        void refreshSidebarSearchCounts(
          current,
          scope,
          barEffective,
          countState,
          { rows, cursor: boolean ? undefined : page.next_cursor, query, sourceErrors: [...sourceErrors] },
          !countsComplete,
        );
    } catch (reason) {
      if (generation === searchGeneration) {
        searchMatchKeys.value = new Set();
        sidebarSearchCounts.value = undefined;
        error.value = reason instanceof Error ? reason.message : String(reason);
      }
    }
  }
  // Every TicketSearchField derives its own `tag:` suggestions from this sorted, canonical-case
  // tag list; it is recomputed only when the ticket collection changes (HS2-N5G6JS).
  let availableSearchTagsSource: WireTicketRow[] | undefined,
    availableSearchTagsValue: string[] = [];
  function availableSearchTags() {
    if (availableSearchTagsSource !== tickets.value) {
      availableSearchTagsSource = tickets.value;
      availableSearchTagsValue = [...new Set(tickets.value.flatMap((ticket) => ticket.tags))].sort((a, b) =>
        a.localeCompare(b),
      );
    }
    return availableSearchTagsValue;
  }
  function focusWorkspaceSearch(offset?: number) {
    restoreInlineSearchCaret(document, '[data-search-field-editor="workspace-search"]', offset);
  }
  /** Publish typed search text still waiting for a typing pause, before reading the projections. */
  function flushWorkspaceSearch() {
    searchProjection.flush();
  }
  function scheduleTicketSearch(delay = 150) {
    resetProgressiveTicketRendering();
    scheduleProjectSessionPersistence();
    searchGeneration += 1;
    sidebarSearchCounts.value = undefined;
    // All Tickets lasts only while the search does (HS2-4J6764).
    if (!searchBarActive() && searchAllTickets.peek()) searchAllTickets.value = false;
    if (searchTimer !== undefined) window.clearTimeout(searchTimer);
    if (!workspaceSearchActive()) {
      searchTimer = undefined;
      searchMatchKeys.value = undefined;
      return;
    }
    searchMatchKeys.value = undefined;
    searchTimer = window.setTimeout(() => {
      searchTimer = undefined;
      void refreshTicketSearch();
    }, delay);
  }
  return {
    searchBarActive,
    workspaceSearchActive,
    searchScope,
    allTicketsSearchAvailable,
    setAllTicketsSearch,
    selectedViewSearch,
    workspaceSearchModel,
    workspaceSearchTokenOffset,
    searchSignature,
    sortedTicketQuery,
    refreshTicketSearch,
    availableSearchTags,
    focusWorkspaceSearch,
    scheduleTicketSearch,
    flushWorkspaceSearch,
  };
}
