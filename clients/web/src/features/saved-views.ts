import { batch, type Signal, signal } from 'kerfjs';

import { Api, type CustomView } from '../api';
import type { SavedViewContextMenuState } from '../components/view-navigation';
import { consumeSearchTokens, type InlineSearchToken, orderedSearchText } from '../inline-search';
import { restoreInlineSearchCaret } from '../inline-search-caret';
import { data } from '../interactions/dom';
import type { Control, Project } from '../interactions/types';
import { customViewNameAvailable, uniqueCustomViewId } from '../saved-views';
import { customTicketViewId, customTicketViewKey, type TicketView } from '../ticket-views';

export interface SavedViewsControllerDependencies {
  readonly project: () => Project | undefined;
  readonly customViewsByProject: Signal<Record<string, CustomView[]>>;
  readonly customViewsFor: (projectId?: string) => CustomView[];
  readonly customViewFor: (view: TicketView) => CustomView | undefined;
  readonly selectedView: Signal<TicketView>;
  readonly searchQuery: Signal<string>;
  readonly searchTokens: Signal<InlineSearchToken[]>;
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
      selectTicketView,
      showToast,
    } = dependencies,
    savedViewDialogOpen = signal(false),
    savedViewDialogMode = signal<'create' | 'rename'>('create'),
    savedViewTargetId = signal<string | undefined>(undefined),
    savedViewName = signal(''),
    savedViewQuery = signal(''),
    savedViewQueryTokens = signal<InlineSearchToken[]>([]),
    savedViewBusy = signal(false),
    savedViewError = signal(''),
    savedViewMenu = signal<SavedViewContextMenuState | undefined>(undefined),
    savedViewDeleteTargetId = signal<string | undefined>(undefined),
    savedViewDeleteBusy = signal(false),
    savedViewDeleteError = signal('');

  function showSavedViewDialog() {
    savedViewDialogOpen.value = true;
    const name = document.querySelector<Control>('[name="saved-view-name"]');
    if (name && name.value !== savedViewName.value) name.value = savedViewName.value;
  }
  function setSavedViewQuery(value: string) {
    const parsed = consumeSearchTokens(value, true);
    savedViewQuery.value = parsed.text;
    savedViewQueryTokens.value = parsed.tokens;
  }
  function openSavedViewDialog() {
    const selected = customViewFor(selectedView.value),
      query = selected?.query ?? orderedSearchText(searchQuery.value, searchTokens.value, () => true);
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
  }
  function updateSavedViewQuery(
    value: string,
    forceToken = false,
    currentTokens = savedViewQueryTokens.value,
    commitToken = forceToken,
  ) {
    const parsed = consumeSearchTokens(value, commitToken),
      shifted = currentTokens.map((token) => {
        const offset = token.offset ?? value.length,
          shift = parsed.removed.reduce(
            (total, range) => total + (range.end <= offset ? range.end - range.start : 0),
            0,
          );
        return { ...token, offset: Math.max(0, offset - shift) };
      }),
      next: InlineSearchToken[] = [...shifted];
    for (const token of parsed.tokens)
      if (!next.some((value) => value.kind === token.kind && value.value === token.value)) next.push(token);
    savedViewQuery.value = parsed.text;
    savedViewQueryTokens.value = next;
    savedViewError.value = '';
    return parsed.tokens.length > 0;
  }
  function focusSavedViewQuery(offset?: number) {
    restoreInlineSearchCaret(document, '[data-token-search-editor="saved-view-query"]', offset);
  }
  function removeSavedViewQueryToken(raw: string) {
    const token = savedViewQueryTokens.value.find((value) => value.raw === raw);
    if (!token) return false;
    const offset = token.offset ?? savedViewQuery.value.length;
    savedViewQueryTokens.value = savedViewQueryTokens.value.filter((value) => value !== token);
    savedViewError.value = '';
    focusSavedViewQuery(offset);
    return true;
  }
  function editSavedViewQueryToken(event: Event, target: Element) {
    event.preventDefault();
    const raw = data(target).tokenValue,
      token = savedViewQueryTokens.value.find((value) => value.raw === raw);
    if (!raw || !token) return;
    const offset = Math.max(0, Math.min(savedViewQuery.value.length, token.offset ?? savedViewQuery.value.length)),
      before = savedViewQuery.value.slice(0, offset),
      after = savedViewQuery.value.slice(offset),
      leading = before && !/[\s(]$/.test(before) ? ' ' : '',
      trailing = after && !/^[\s)]/.test(after) ? ' ' : '',
      insert = `${leading}${raw}${trailing}`;
    batch(() => {
      savedViewQueryTokens.value = savedViewQueryTokens.value
        .filter((value) => value.raw !== raw)
        .map((value) => ({
          ...value,
          offset:
            (value.offset ?? savedViewQuery.value.length) >= offset
              ? (value.offset ?? savedViewQuery.value.length) + insert.length
              : value.offset,
        }));
      savedViewQuery.value = before + insert + after;
    });
    focusSavedViewQuery(offset + leading.length + raw.length);
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
    savedViewDialogMode,
    savedViewTargetId,
    savedViewName,
    savedViewQuery,
    savedViewQueryTokens,
    savedViewBusy,
    savedViewError,
    savedViewMenu,
    savedViewDeleteTargetId,
    savedViewDeleteBusy,
    savedViewDeleteError,
    openSavedViewDialog,
    openSavedViewRename,
    closeSavedViewDialog,
    updateSavedViewQuery,
    focusSavedViewQuery,
    removeSavedViewQueryToken,
    editSavedViewQueryToken,
    saveSavedView,
    openSavedViewDelete,
    closeSavedViewDelete,
    deleteSavedView,
  };
}
