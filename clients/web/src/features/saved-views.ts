import { effect, type Signal, signal } from 'kerfjs';

import { Api, type CustomView } from '../api';
import type { SavedViewContextMenuState } from '../components/view-navigation';
import { type InlineSearchToken, orderedSearchText } from '../inline-search';
import { restoreInlineSearchCaret } from '../inline-search-caret';
import type { Control, Project } from '../interactions/types';
import { customViewNameAvailable, customViewQueryText, uniqueCustomViewId } from '../saved-views';
import { createTicketSearchModel, inlineSearchTokens, replaceTicketSearch } from '../ticket-search-model';
import { customTicketViewId, customTicketViewKey, type TicketView } from '../ticket-views';

export interface SavedViewsControllerDependencies {
  readonly project: () => Project | undefined;
  readonly customViewsByProject: Signal<Record<string, CustomView[]>>;
  readonly customViewsFor: (projectId?: string) => CustomView[];
  readonly customViewFor: (view: TicketView) => CustomView | undefined;
  readonly selectedView: Signal<TicketView>;
  readonly searchQuery: Signal<string>;
  readonly searchTokens: Signal<InlineSearchToken[]>;
  /** Every project tag, canonical casing, for the query field's tag completion. */
  readonly availableSearchTags: () => readonly string[];
  readonly selectTicketView: (view: TicketView) => void;
  readonly showToast: (message: string) => void;
}

/** Shared-view dialog state and persistence, isolated from application bootstrap. */
export function createSavedViewsController(dependencies: SavedViewsControllerDependencies) {
  const {
      project,
      customViewsByProject,
      customViewsFor,
      customViewFor,
      selectedView,
      searchQuery,
      searchTokens,
      availableSearchTags,
      selectTicketView,
      showToast,
    } = dependencies,
    savedViewDialogOpen = signal(false),
    savedViewDialogSession = signal(0),
    savedViewDialogMode = signal<'create' | 'rename'>('create'),
    savedViewTargetId = signal<string | undefined>(undefined),
    savedViewName = signal(''),
    savedViewQuery = signal(''),
    savedViewQueryTokens = signal<InlineSearchToken[]>([]),
    savedViewHelpOpen = signal(false),
    savedViewBusy = signal(false),
    savedViewError = signal(''),
    savedViewMenu = signal<SavedViewContextMenuState | undefined>(undefined),
    savedViewDeleteTargetId = signal<string | undefined>(undefined),
    savedViewDeleteBusy = signal(false),
    savedViewDeleteError = signal('');
  // Kerf's managed TokenSearchModel owns the dialog's query text, chips, and tag completion
  // (HS2-5JXBQY); `savedViewQuery`/`savedViewQueryTokens` are projections of its state.
  const savedViewSearchModel = createTicketSearchModel({
    tags: availableSearchTags,
    onClear: () => {
      savedViewHelpOpen.value = false;
      savedViewError.value = '';
    },
  });
  effect(() => {
    const state = savedViewSearchModel.state.value;
    savedViewQuery.value = state.query;
    savedViewQueryTokens.value = inlineSearchTokens(state);
    savedViewError.value = '';
  });
  /** The caret position a chip occupies, for focus restoration around Kerf's chip actions. */
  function savedViewSearchTokenOffset(raw: string) {
    return savedViewSearchModel.state.value.tokens.find((token) => token.value === raw)?.offset;
  }

  function showSavedViewDialog() {
    savedViewDialogSession.value += 1;
    savedViewDialogOpen.value = true;
    const name = document.querySelector<Control>('[name="saved-view-name"]');
    if (name && name.value !== savedViewName.value) name.value = savedViewName.value;
  }
  function setSavedViewQuery(value: string) {
    replaceTicketSearch(savedViewSearchModel, value);
    savedViewHelpOpen.value = false;
  }
  function openSavedViewDialog() {
    const selected = customViewFor(selectedView.value),
      // A new view starts from what is visible: the selected view scoped by any search-bar query (HS2-50R1YQ).
      query = selected
        ? customViewQueryText(selected, searchQuery.value, searchTokens.value)
        : orderedSearchText(searchQuery.value, searchTokens.value, () => true);
    savedViewDialogMode.value = 'create';
    savedViewTargetId.value = undefined;
    savedViewName.value = '';
    setSavedViewQuery(query);
    savedViewError.value = '';
    savedViewBusy.value = false;
    showSavedViewDialog();
  }
  function openSavedViewRename(viewId: string) {
    const id = customTicketViewKey(viewId as TicketView),
      view = id && customViewsFor().find((item) => item.id === id);
    if (!view) return;
    savedViewDialogMode.value = 'rename';
    savedViewTargetId.value = view.id;
    savedViewName.value = view.name;
    setSavedViewQuery(view.query);
    savedViewError.value = '';
    savedViewBusy.value = false;
    savedViewMenu.value = undefined;
    showSavedViewDialog();
  }
  function closeSavedViewDialog() {
    if (savedViewBusy.value) return;
    savedViewDialogOpen.value = false;
    savedViewTargetId.value = undefined;
    savedViewError.value = '';
    savedViewHelpOpen.value = false;
  }
  /** Replace the trailing uncommitted filter text matched by `pattern` with a chip at that position. */
  function focusSavedViewQuery(offset?: number) {
    restoreInlineSearchCaret(document, '[data-token-search-editor="saved-view-query"]', offset);
  }
  async function saveSavedView(form: HTMLFormElement) {
    const current = project();
    if (!current || savedViewBusy.value) return;
    const existing = customViewsFor(current.id),
      targetId = savedViewDialogMode.value === 'rename' ? savedViewTargetId.value : undefined,
      target = targetId ? existing.find((item) => item.id === targetId) : undefined,
      name = (form.querySelector<Control>('[name="saved-view-name"]')?.value ?? '').trim(),
      query = orderedSearchText(savedViewQuery.value, savedViewQueryTokens.value, () => true).trim();
    if (targetId && !target) savedViewError.value = 'That shared view no longer exists.';
    else if (!name) savedViewError.value = 'Enter a view name.';
    else if (name.length > 80) savedViewError.value = 'View names can be at most 80 characters.';
    else if (!customViewNameAvailable(name, existing, targetId))
      savedViewError.value = 'That view name is already in use.';
    else if (!query) savedViewError.value = 'Enter a search query.';
    else if (query.length > 2_000) savedViewError.value = 'Search queries can be at most 2,000 characters.';
    else {
      const savedView: CustomView = target
          ? { ...target, name, query }
          : { id: uniqueCustomViewId(name, existing), name, query },
        next = target ? existing.map((item) => (item.id === target.id ? savedView : item)) : [...existing, savedView];
      savedViewBusy.value = true;
      savedViewError.value = '';
      try {
        const saved = await new Api(current.apiPath).saveCustomViews(next);
        if (project()?.id !== current.id) return;
        customViewsByProject.value = { ...customViewsByProject.value, [current.id]: saved };
        savedViewDialogOpen.value = false;
        selectTicketView(customTicketViewId(savedView.id));
        showToast(`${target ? 'Updated' : 'Created'} ${savedView.name}.`);
      } catch (reason) {
        if (project()?.id === current.id)
          savedViewError.value = reason instanceof Error ? reason.message : String(reason);
      } finally {
        if (project()?.id === current.id) savedViewBusy.value = false;
      }
    }
  }
  function openSavedViewDelete(viewId: string) {
    const id = customTicketViewKey(viewId as TicketView);
    if (!id || !customViewsFor().some((item) => item.id === id)) return;
    savedViewDeleteTargetId.value = id;
    savedViewDeleteBusy.value = false;
    savedViewDeleteError.value = '';
    requestAnimationFrame(() =>
      requestAnimationFrame(() =>
        document.querySelector<Control>('[data-component="saved-view-delete-dialog"]')?.show?.(),
      ),
    );
  }
  function closeSavedViewDelete() {
    if (savedViewDeleteBusy.value) return;
    savedViewDeleteTargetId.value = undefined;
    savedViewDeleteError.value = '';
  }
  async function deleteSavedView() {
    const current = project(),
      targetId = savedViewDeleteTargetId.value;
    if (!current || !targetId || savedViewDeleteBusy.value) return;
    const existing = customViewsFor(current.id),
      target = existing.find((item) => item.id === targetId);
    if (!target) {
      savedViewDeleteTargetId.value = undefined;
      return;
    }
    savedViewDeleteBusy.value = true;
    savedViewDeleteError.value = '';
    try {
      const saved = await new Api(current.apiPath).saveCustomViews(existing.filter((item) => item.id !== targetId));
      if (project()?.id !== current.id) return;
      customViewsByProject.value = { ...customViewsByProject.value, [current.id]: saved };
      savedViewDeleteTargetId.value = undefined;
      if (selectedView.value === customTicketViewId(targetId)) selectTicketView('all');
      showToast(`Deleted ${target.name}.`);
    } catch (reason) {
      if (project()?.id === current.id)
        savedViewDeleteError.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (project()?.id === current.id) savedViewDeleteBusy.value = false;
    }
  }

  return {
    savedViewDialogOpen,
    savedViewDialogSession,
    savedViewDialogMode,
    savedViewTargetId,
    savedViewName,
    savedViewQuery,
    savedViewQueryTokens,
    savedViewHelpOpen,
    savedViewBusy,
    savedViewError,
    savedViewMenu,
    savedViewDeleteTargetId,
    savedViewDeleteBusy,
    savedViewDeleteError,
    openSavedViewDialog,
    openSavedViewRename,
    closeSavedViewDialog,
    savedViewSearchModel,
    savedViewSearchTokenOffset,
    focusSavedViewQuery,
    saveSavedView,
    openSavedViewDelete,
    closeSavedViewDelete,
    deleteSavedView,
  };
}
