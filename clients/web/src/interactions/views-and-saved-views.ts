import { delegate, delegateCapture, type Signal } from 'kerfjs';

import { type FullTicket } from '../api';
import { type SavedViewContextMenuState } from '../components/view-navigation';
import { viewportSafeContextMenuPosition } from '../context-menu-position';
import { type InlineSearchToken } from '../inline-search';
import { type TicketView } from '../ticket-views';
import { data } from './dom';
import { type Control } from './types';

/** Live application bindings used by this handler group. */
export interface ViewAndSavedViewInteractionsDependencies {
  readonly selectedCorruptKey: Signal<string | undefined>;
  readonly selectedTicketSlugs: Signal<string[]>;
  ticketSelectionAnchor: string | undefined;
  readonly selectedTicket: Signal<FullTicket | null>;
  readonly selectTicketView: (next: TicketView, { refresh }?: { refresh?: boolean }) => void;
  readonly isEditableEvent: (event: Event) => boolean;
  readonly openSavedViewDialog: () => void;
  readonly savedViewMenu: Signal<SavedViewContextMenuState | undefined>;
  readonly openSavedViewRename: (viewId: string) => void;
  readonly openSavedViewDelete: (viewId: string) => void;
  readonly savedViewName: Signal<string>;
  readonly savedViewError: Signal<string>;
  readonly readInlineSearchField: (
    editor: HTMLElement,
    current: readonly InlineSearchToken[],
  ) => { text: string; tokens: InlineSearchToken[] };
  readonly savedViewQueryTokens: Signal<InlineSearchToken[]>;
  readonly updateSavedViewQuery: (
    value: string,
    forceToken?: boolean,
    currentTokens?: InlineSearchToken[],
    commitToken?: boolean,
  ) => boolean;
  readonly focusSavedViewQuery: (offset?: number) => void;
  readonly removeSavedViewQueryToken: (raw: string) => boolean;
  readonly editSavedViewQueryToken: (event: Event, target: Element) => void;
  readonly savedViewQuery: Signal<string>;
  readonly saveSavedView: (form: HTMLFormElement) => Promise<void>;
  readonly closeSavedViewDialog: () => void;
  readonly savedViewBusy: Signal<boolean>;
  readonly deleteSavedView: () => Promise<void>;
  readonly closeSavedViewDelete: () => void;
  readonly savedViewDeleteBusy: Signal<boolean>;
}

/** Register this group only when the application wiring owner invokes it. */
export function wireViewAndSavedViewInteractions(dependencies: ViewAndSavedViewInteractionsDependencies) {
  const {
    selectedCorruptKey,
    selectedTicketSlugs,
    selectedTicket,
    selectTicketView,
    isEditableEvent,
    openSavedViewDialog,
    savedViewMenu,
    openSavedViewRename,
    openSavedViewDelete,
    savedViewName,
    savedViewError,
    readInlineSearchField,
    savedViewQueryTokens,
    updateSavedViewQuery,
    focusSavedViewQuery,
    removeSavedViewQueryToken,
    editSavedViewQueryToken,
    savedViewQuery,
    saveSavedView,
    closeSavedViewDialog,
    savedViewBusy,
    deleteSavedView,
    closeSavedViewDelete,
    savedViewDeleteBusy,
  } = dependencies;
  delegate(document.body, 'click', '[data-ticket-selection-root="true"]', (event) => {
    const pointer = event as MouseEvent;
    if (
      (event.target as Element).closest('[data-action="select-ticket-row"]') ||
      pointer.shiftKey ||
      pointer.metaKey ||
      pointer.ctrlKey
    )
      return;
    selectedCorruptKey.value = undefined;
    selectedTicketSlugs.value = [];
    dependencies.ticketSelectionAnchor = undefined;
    selectedTicket.value = null;
  });
  delegate(document.body, 'click', '[data-action="select-view"]', (_event, target) => {
    selectTicketView((data(target).itemId ?? 'all') as TicketView);
  });
  delegateCapture(document.body, 'click', '[data-action="add-view"]', (event) => {
    if (isEditableEvent(event)) return;
    openSavedViewDialog();
  });
  delegate(document.body, 'click', '[data-action="open-saved-view-menu"]', (event, target) => {
    event.stopPropagation();
    const rect = target.getBoundingClientRect(),
      id = data(target).itemId,
      label = data(target).itemLabel;
    if (!id || !label) return;
    savedViewMenu.value = {
      id,
      label,
      ...viewportSafeContextMenuPosition(rect.right, rect.bottom, window.innerWidth, window.innerHeight, {
        width: 192,
        height: 96,
      }),
    };
  });
  delegate(document.body, 'contextmenu', '[data-saved-view-id]', (event, target) => {
    event.preventDefault();
    const row = target.closest<HTMLElement>('.view-navigation__item') ?? (target as HTMLElement),
      id = data(row).savedViewId,
      label = data(row).savedViewLabel,
      pointer = event as MouseEvent;
    if (!id || !label) return;
    savedViewMenu.value = {
      id,
      label,
      ...viewportSafeContextMenuPosition(pointer.clientX, pointer.clientY, window.innerWidth, window.innerHeight, {
        width: 192,
        height: 96,
      }),
    };
  });
  delegate(document.body, 'click', '[data-action="edit-saved-view"]', (event, target) => {
    event.stopPropagation();
    savedViewMenu.value = undefined;
    openSavedViewRename(data(target).itemId!);
  });
  delegate(document.body, 'click', '[data-action="delete-saved-view"]', (event, target) => {
    event.stopPropagation();
    savedViewMenu.value = undefined;
    openSavedViewDelete(data(target).itemId!);
  });
  delegate(document.body, 'input', '[name="saved-view-name"]', (_event, target) => {
    savedViewName.value = (target as Control).value;
    savedViewError.value = '';
  });
  delegate(document.body, 'input', '[data-token-search-editor="saved-view-query"]', (event, target) => {
    const editor = target as HTMLElement,
      state = readInlineSearchField(editor, savedViewQueryTokens.value),
      input = event as InputEvent,
      commitsToken =
        (typeof input.data === 'string' && /\s$/.test(input.data)) ||
        (input.inputType === 'insertFromPaste' && /\s$/.test(state.text));
    if (updateSavedViewQuery(state.text, false, state.tokens, commitsToken)) focusSavedViewQuery();
  });
  delegate(document.body, 'mousedown', '[data-action="clear-saved-view-query"]', (event) => {
    event.preventDefault();
  });
  delegate(document.body, 'click', '[data-action="remove-saved-view-query-token"]', (_event, target) => {
    const raw = data(target).tokenValue;
    if (raw) removeSavedViewQueryToken(raw);
  });
  delegate(document.body, 'click', '[data-action="edit-saved-view-query-token"]', editSavedViewQueryToken);
  delegate(
    document.body,
    'dblclick',
    '[data-token-search-editor="saved-view-query"] [data-component="token-search-token"]',
    editSavedViewQueryToken,
  );
  delegate(document.body, 'click', '[data-action="clear-saved-view-query"]', () => {
    const editor = document.querySelector<HTMLElement>('[data-token-search-editor="saved-view-query"]');
    if (editor) editor.textContent = '';
    savedViewQuery.value = '';
    savedViewQueryTokens.value = [];
    savedViewError.value = '';
    focusSavedViewQuery(0);
  });
  delegate(document.body, 'submit', '[data-action="save-saved-view"]', (event, target) => {
    event.preventDefault();
    void saveSavedView(target as HTMLFormElement);
  });
  delegate(document.body, 'click', '[data-action="cancel-saved-view"]', () => {
    closeSavedViewDialog();
  });
  delegate(document.body, 'wa-hide', '[data-component="saved-view-dialog"]', (event) => {
    if (savedViewBusy.value) {
      event.preventDefault();
      return;
    }
    closeSavedViewDialog();
  });
  delegate(document.body, 'click', '[data-action="confirm-delete-saved-view"]', () => {
    void deleteSavedView();
  });
  delegate(document.body, 'click', '[data-action="cancel-delete-saved-view"]', () => {
    closeSavedViewDelete();
  });
  delegate(document.body, 'wa-hide', '[data-component="saved-view-delete-dialog"]', (event) => {
    if (savedViewDeleteBusy.value) {
      event.preventDefault();
      return;
    }
    closeSavedViewDelete();
  });
}
