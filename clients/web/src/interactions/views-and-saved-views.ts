import { delegate, delegateCapture, effect, type Signal } from 'kerfjs';

import { type FullTicket } from '../api';
import { type SavedViewContextMenuState } from '../components/view-navigation';
import { revealContextPopupMenu, viewportSafeContextMenuPosition } from '../context-menu-position';
import { type TicketView } from '../ticket-views';
import { data } from './dom';
import { createInteractionLifetime } from './lifetime';
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
  readonly saveSavedView: (form: HTMLFormElement) => Promise<void>;
  readonly closeSavedViewDialog: () => void;
  readonly savedViewBusy: Signal<boolean>;
  readonly deleteSavedView: () => Promise<void>;
  readonly closeSavedViewDelete: () => void;
  readonly savedViewDeleteBusy: Signal<boolean>;
}

/** Register this group only when the application wiring owner invokes it. */
export function wireViewAndSavedViewInteractions(dependencies: ViewAndSavedViewInteractionsDependencies) {
  const lifetime = createInteractionLifetime();
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
    saveSavedView,
    closeSavedViewDialog,
    savedViewBusy,
    deleteSavedView,
    closeSavedViewDelete,
    savedViewDeleteBusy,
  } = dependencies;
  lifetime.add(
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
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', '[data-action="select-view"]', (_event, target) => {
      selectTicketView((data(target).itemId ?? 'all') as TicketView);
    }),
  );
  lifetime.add(
    delegateCapture(document.body, 'click', '[data-action="add-view"]', (event) => {
      if (isEditableEvent(event)) return;
      openSavedViewDialog();
    }),
  );
  lifetime.add(
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
    }),
  );
  effect(() => {
    // Partial dependencies in the handler-transition tests may omit this signal.
    if ((savedViewMenu as typeof savedViewMenu | undefined)?.value) revealContextPopupMenu('saved-view');
  });
  lifetime.add(
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
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', '[data-action="edit-saved-view"]', (event, target) => {
      event.stopPropagation();
      savedViewMenu.value = undefined;
      openSavedViewRename(data(target).itemId!);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', '[data-action="delete-saved-view"]', (event, target) => {
      event.stopPropagation();
      savedViewMenu.value = undefined;
      openSavedViewDelete(data(target).itemId!);
    }),
  );
  lifetime.add(
    delegate(document.body, 'input', '[name="saved-view-name"]', (_event, target) => {
      savedViewName.value = (target as Control).value;
      savedViewError.value = '';
    }),
  );
  lifetime.add(
    delegate(document.body, 'submit', '[data-action="save-saved-view"]', (event, target) => {
      event.preventDefault();
      void saveSavedView(target as HTMLFormElement);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', '[data-action="cancel-saved-view"]', () => {
      closeSavedViewDialog();
    }),
  );
  lifetime.add(
    delegate(document.body, 'wa-hide', '[data-component="saved-view-dialog"]', (event) => {
      if (savedViewBusy.value) {
        event.preventDefault();
        return;
      }
      closeSavedViewDialog();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', '[data-action="confirm-delete-saved-view"]', () => {
      void deleteSavedView();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', '[data-action="cancel-delete-saved-view"]', () => {
      closeSavedViewDelete();
    }),
  );
  lifetime.add(
    delegate(document.body, 'wa-hide', '[data-component="saved-view-delete-dialog"]', (event) => {
      if (savedViewDeleteBusy.value) {
        event.preventDefault();
        return;
      }
      closeSavedViewDelete();
    }),
  );
  return lifetime.dispose;
}
