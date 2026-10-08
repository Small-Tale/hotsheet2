import { batch, delegate, delegateCapture, type Signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { type FullTicket, type TicketCloseReason, type TicketRow as WireTicketRow } from '../api';
import { type BulkTicketDialogState } from '../components/bulk-ticket-dialog';
import { type TicketCloseDialogState } from '../components/ticket-close-dialog';
import { adjacentTicketSlug, isPlainTicketReselection, selectAllTickets } from '../components/ticket-selection';
import { wireWorkspaceOverflowKeyboard } from '../components/workspace-header';
import { viewportSafeContextMenuPosition, viewportSafePointerPosition } from '../context-menu-position';
import { TICKET_SELECTION_ACTIONS, TICKET_SELECTION_TARGETS } from '../interaction-attrs/ticket-selection';
import { matchesShortcut, type ShortcutChord } from '../keyboard-shortcuts';
import { shouldAutoOpenInspectorOnTap } from '../mobile-layout';
import { type BulkTicketAction } from '../ticket-bulk-operations';
import { duplicateTargetKey } from '../ticket-close';
import { deleteDraftFiles } from '../workspace-session';
import { data } from './dom';
import { type Control, type NotWorkingTarget, type PendingEvidence } from './types';

/** Live application bindings used by this handler group. */
export interface TicketSelectionInteractionsDependencies {
  readonly viewportMobile: Signal<boolean>;
  /** Open the inspector overlay on a phone (Kerf closes the sidebar); a no-op on larger layouts. */
  readonly revealInspectorOverlay: () => void;
  readonly selectTickets: (
    slug: string,
    intent?: { range?: boolean; toggle?: boolean },
    ordered?: string[],
  ) => Promise<FullTicket | null> | Promise<(FullTicket & { store: string }) | undefined>;
  readonly selectionOrder: (target: Element) => string[];
  readonly selectedTicketSlugs: Signal<string[]>;
  readonly terminalRailScreen: Signal<'root' | 'ticket'>;
  readonly selectedTicket: Signal<FullTicket | null>;
  readonly visibleTickets: () => WireTicketRow[];
  readonly cancelTicketDrafts: () => void;
  readonly selectedCorruptKey: Signal<string | undefined>;
  ticketSelectionAnchor: string | undefined;
  readonly setInspectorVisible: (visible: boolean) => void;
  readonly openTicketReader: (slug: string, ordered?: string[], trigger?: HTMLElement) => Promise<void>;
  readonly ticketContextMenu: Signal<{ x: number; y: number; ticketSlug: string; hideUpNext?: boolean } | undefined>;
  readonly selectedRows: () => WireTicketRow[];
  readonly executeBulkTicketAction: (action: BulkTicketAction, targetSlugs?: string[]) => Promise<boolean>;
  readonly tickets: Signal<WireTicketRow[]>;
  readonly openNotWorking: (ticket: WireTicketRow, mode?: NotWorkingTarget['mode']) => void;
  readonly openTicketClose: (ticket: WireTicketRow) => void;
  readonly copySelection: (cut: boolean) => void;
  readonly pasteSelection: () => Promise<void>;
  readonly openBulkTicketDialog: (kind: 'add-tag' | 'remove-tag' | 'delete', targetSlugs?: string[]) => void;
  readonly restoreTrashedTickets: (targetSlugs: readonly string[]) => Promise<void>;
  bulkTicketSlugs: string[];
  readonly bulkTicketDialog: Signal<BulkTicketDialogState | undefined>;
  readonly openEmptyTrash: () => void;
  readonly emptyTrash: () => Promise<void>;
  readonly setTicketCloseReason: (reason: TicketCloseReason) => void;
  readonly searchTicketCloseTargets: (query: string) => void;
  readonly ticketCloseDialog: Signal<TicketCloseDialogState | undefined>;
  readonly submitTicketClose: () => Promise<void>;
  readonly closeTicketCloseDialog: () => void;
  ticketLinkReturnFocus: HTMLElement | undefined;
  readonly openDuplicateTarget: (id: string) => Promise<void>;
  readonly notWorkingNote: Signal<string>;
  readonly scheduleProjectSessionPersistence: () => void;
  readonly presentNotWorkingDialog: () => void;
  readonly addNotWorkingFiles: (files: FileList | File[]) => Promise<void>;
  readonly draftScope: (kind: 'composer' | 'not-working', projectId?: string) => string;
  readonly notWorkingTarget: Signal<NotWorkingTarget>;
  readonly notWorkingFiles: Signal<PendingEvidence[]>;
  readonly submitNotWorking: () => Promise<void>;
  readonly closeNotWorking: (clearStored?: boolean) => void;
  readonly notWorkingSubmitting: Signal<boolean>;
  readonly keyboardShortcutOverrides: Signal<Record<string, ShortcutChord>>;
  readonly appleShortcutPlatform: boolean;
}

/** Register this group only when the application wiring owner invokes it. */
export function wireTicketSelectionInteractions(dependencies: TicketSelectionInteractionsDependencies) {
  const lifetime = createScope();
  const {
    viewportMobile,
    revealInspectorOverlay,
    selectTickets,
    selectionOrder,
    selectedTicketSlugs,
    terminalRailScreen,
    selectedTicket,
    visibleTickets,
    cancelTicketDrafts,
    selectedCorruptKey,
    setInspectorVisible,
    openTicketReader,
    ticketContextMenu,
    selectedRows,
    executeBulkTicketAction,
    tickets,
    openNotWorking,
    openTicketClose,
    copySelection,
    pasteSelection,
    openBulkTicketDialog,
    restoreTrashedTickets,
    bulkTicketDialog,
    openEmptyTrash,
    emptyTrash,
    setTicketCloseReason,
    searchTicketCloseTargets,
    ticketCloseDialog,
    submitTicketClose,
    closeTicketCloseDialog,
    openDuplicateTarget,
    notWorkingNote,
    scheduleProjectSessionPersistence,
    presentNotWorkingDialog,
    addNotWorkingFiles,
    draftScope,
    notWorkingTarget,
    notWorkingFiles,
    submitNotWorking,
    closeNotWorking,
    notWorkingSubmitting,
    keyboardShortcutOverrides,
    appleShortcutPlatform,
  } = dependencies;
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_ACTIONS.selectTicketRow.selector, (event, target) => {
      if ((event.target as Element).closest('[data-action="toggle-row-up-next"]')) return;
      const pointer = event as MouseEvent,
        rail = Boolean(target.closest('[data-component="terminal-ticket-rail-list"]'));
      // Mobile has no persistent side inspector, so a plain tap on a workspace-list ticket auto-opens the
      // right inspector overlay (tap-away on the scrim returns to the list) — HS2-N7RPFP. Range/toggle
      // multi-select taps and the terminal rail are excluded.
      if (
        shouldAutoOpenInspectorOnTap({
          mobile: viewportMobile.value,
          rail,
          shiftKey: pointer.shiftKey,
          metaKey: pointer.metaKey,
          ctrlKey: pointer.ctrlKey,
        })
      )
        revealInspectorOverlay();
      void selectTickets(
        data(target).ticketSlug!,
        { range: pointer.shiftKey, toggle: pointer.metaKey || pointer.ctrlKey },
        selectionOrder(target),
      ).then((ticket) => {
        if (rail && ticket && selectedTicketSlugs.value.length === 1) {
          terminalRailScreen.value = 'ticket';
        }
      });
    }),
  );
  lifetime.add(
    delegateCapture(
      document.body,
      'pointerdown',
      TICKET_SELECTION_ACTIONS.selectTicketRow.selector,
      (event, target) => {
        const pointer = event as PointerEvent,
          active = document.activeElement,
          slug = data(target).ticketSlug!;
        if (
          pointer.button === 0 &&
          active instanceof HTMLElement &&
          active.closest(
            '[data-component="ticket-inspector-header"], [data-component="ticket-inspector-body"], [data-component="ticket-reader"]',
          ) &&
          active.matches(
            'input, textarea, select, wa-input, wa-textarea, wa-select, [role="textbox"], [contenteditable]:not([contenteditable="false"])',
          ) &&
          isPlainTicketReselection(selectedTicketSlugs.value, selectedTicket.value?.slug, slug, {
            range: pointer.shiftKey,
            toggle: pointer.metaKey || pointer.ctrlKey,
          })
        )
          event.preventDefault();
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_ACTIONS.selectTicketColumn.selector, (event, target) => {
      event.stopImmediatePropagation();
      const column = target.closest<HTMLElement>('[data-component="ticket-board-column"]'),
        columnId = column?.dataset.columnId;
      if (!columnId) return;
      const slugs = [...column.querySelectorAll<HTMLElement>('[data-action="select-ticket-row"]')].map(
        (ticket) => data(ticket).ticketSlug!,
      );
      cancelTicketDrafts();
      selectedCorruptKey.value = undefined;
      selectedTicketSlugs.value = slugs;
      dependencies.ticketSelectionAnchor = slugs[0];
      selectedTicket.value = null;
      setInspectorVisible(true);
    }),
  );
  lifetime.add(
    delegate(document.body, 'dblclick', TICKET_SELECTION_ACTIONS.selectTicketRow.selector, (event, target) => {
      if ((event.target as Element).closest('button, input, textarea, select, a, [contenteditable="true"]')) return;
      void openTicketReader(data(target).ticketSlug!, selectionOrder(target), target as HTMLElement);
    }),
  );
  lifetime.add(
    delegate(document.body, 'contextmenu', TICKET_SELECTION_ACTIONS.selectTicketRow.selector, (event, target) => {
      event.preventDefault();
      const pointer = event as MouseEvent,
        slug = data(target).ticketSlug!;
      if (!selectedTicketSlugs.value.includes(slug)) void selectTickets(slug, {}, selectionOrder(target));
      ticketContextMenu.value = {
        ...viewportSafePointerPosition(pointer.clientX, pointer.clientY, window.innerWidth, window.innerHeight),
        ticketSlug: slug,
      };
    }),
  );
  lifetime.add(wireWorkspaceOverflowKeyboard(document.body));
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_ACTIONS.toggleSelectedUpNext.selector, () => {
      const selected = selectedRows();
      if (selected.length === 0) return;
      void executeBulkTicketAction(
        { kind: 'up-next', value: !selected.every((ticket) => ticket.up_next) },
        selected.map((ticket) => ticket.slug),
      );
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_ACTIONS.openSelectedTicketActions.selector, (_event, target) => {
      const selected = selectedRows();
      if (selected.length === 0) return;
      const rect = target.getBoundingClientRect(),
        position = viewportSafeContextMenuPosition(
          rect.right - 232,
          rect.bottom,
          window.innerWidth,
          window.innerHeight,
          {
            width: 232,
            height: 382,
          },
        );
      ticketContextMenu.value = { ...position, ticketSlug: selected[0].slug, hideUpNext: true };
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', '[data-context-field]', (event, target) => {
      event.stopPropagation();
      const menu = ticketContextMenu.value,
        field = data(target).contextField as 'category' | 'priority' | 'status' | undefined,
        value = data(target).contextValue;
      if (!menu || !field || !value) return;
      const slugs = selectedTicketSlugs.value.length ? [...selectedTicketSlugs.value] : [menu.ticketSlug];
      batch(() => {
        ticketContextMenu.value = undefined;
        void executeBulkTicketAction({ kind: 'field', field, value }, slugs);
      });
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_TARGETS.reportNotWorkingContextAction.selector, (event) => {
      event.stopImmediatePropagation();
      const menu = ticketContextMenu.value;
      if (!menu) return;
      const ticket = tickets.value.find((item) => item.slug === menu.ticketSlug);
      ticketContextMenu.value = undefined;
      if (ticket) openNotWorking(ticket);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_TARGETS.reopenTicketContextAction.selector, (event) => {
      event.stopImmediatePropagation();
      const menu = ticketContextMenu.value;
      if (!menu) return;
      const ticket = tickets.value.find((item) => item.slug === menu.ticketSlug);
      ticketContextMenu.value = undefined;
      if (ticket) openNotWorking(ticket, 'reopen');
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_TARGETS.closeTicketContextAction.selector, (event) => {
      event.stopImmediatePropagation();
      const menu = ticketContextMenu.value;
      if (!menu) return;
      const ticket = tickets.value.find((item) => item.slug === menu.ticketSlug);
      ticketContextMenu.value = undefined;
      if (ticket) openTicketClose(ticket);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', '[data-context-action]', (_event, target) => {
      const menu = ticketContextMenu.value;
      if (!menu) return;
      const action = data(target).contextAction,
        slugs = selectedTicketSlugs.value.length ? [...selectedTicketSlugs.value] : [menu.ticketSlug],
        selected = tickets.value.filter((item) => slugs.includes(item.slug)),
        row = document.querySelector<HTMLElement>(
          `[data-action="select-ticket-row"][data-ticket-slug="${CSS.escape(menu.ticketSlug)}"]`,
        );
      ticketContextMenu.value = undefined;
      if (action === 'Open ticket') {
        void openTicketReader(menu.ticketSlug, undefined, row ?? undefined);
        return;
      }
      if (action === 'Verify ticket') {
        void executeBulkTicketAction({ kind: 'field', field: 'status', value: 'verified' }, slugs);
        return;
      }
      if (action === 'Reopen ticket') {
        void executeBulkTicketAction({ kind: 'reopen' }, slugs);
        return;
      }
      if (action === 'Toggle Up Next' && selected.length) {
        void executeBulkTicketAction({ kind: 'up-next', value: !selected.every((ticket) => ticket.up_next) }, slugs);
        return;
      }
      if (action === 'Duplicate ticket') {
        copySelection(false);
        void pasteSelection();
        return;
      }
      if (action === 'Move to Backlog') {
        void executeBulkTicketAction({ kind: 'field', field: 'status', value: 'backlog' }, slugs);
        return;
      }
      if (action === 'Archive ticket') {
        void executeBulkTicketAction({ kind: 'field', field: 'status', value: 'archive' }, slugs);
        return;
      }
      if (action === 'Add tag') {
        openBulkTicketDialog('add-tag', slugs);
        return;
      }
      if (action === 'Remove tag') {
        openBulkTicketDialog('remove-tag', slugs);
        return;
      }
      if (action === 'Delete ticket') {
        openBulkTicketDialog('delete', slugs);
        return;
      }
      if (action === 'Restore ticket') {
        void restoreTrashedTickets(slugs);
        return;
      }
    }),
  );
  lifetime.add(
    delegate(document.body, 'submit', TICKET_SELECTION_ACTIONS.submitBulkTag.selector, (event, target) => {
      event.preventDefault();
      const value = target.querySelector<Control>('[name="bulk-ticket-tag"]')?.value ?? '',
        mode = data(target).tagMode;
      if (mode === 'add' || mode === 'remove')
        void executeBulkTicketAction(
          { kind: mode === 'add' ? 'add-tag' : 'remove-tag', tag: value },
          dependencies.bulkTicketSlugs,
        );
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_ACTIONS.chooseBulkTag.selector, (_event, target) => {
      const input = document.querySelector<Control>('[name="bulk-ticket-tag"]');
      if (input) {
        input.value = data(target).tag ?? '';
        input.focus();
      }
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_ACTIONS.cancelBulkTicketAction.selector, () => {
      bulkTicketDialog.value = undefined;
      dependencies.bulkTicketSlugs = [];
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_ACTIONS.openEmptyTrash.selector, () => {
      openEmptyTrash();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_ACTIONS.confirmEmptyTrash.selector, () => {
      void emptyTrash();
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', TICKET_SELECTION_TARGETS.ticketCloseReasonField.selector, (_event, target) => {
      setTicketCloseReason((target as Control).value as TicketCloseReason);
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'input',
      TICKET_SELECTION_TARGETS.ticketCloseTargetSearchField.selector,
      (_event, target) => {
        searchTicketCloseTargets((target as Control).value);
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_ACTIONS.selectTicketCloseTarget.selector, (_event, target) => {
      const current = ticketCloseDialog.value,
        candidate = current?.candidates.find((item) => duplicateTargetKey(item) === data(target).itemId);
      if (current && candidate)
        ticketCloseDialog.value = { ...current, selected: candidate, query: '', candidates: [], error: '' };
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_ACTIONS.clearTicketCloseTarget.selector, () => {
      const current = ticketCloseDialog.value;
      if (!current) return;
      ticketCloseDialog.value = { ...current, selected: undefined, query: '', candidates: [], error: '' };
      queueMicrotask(() => document.querySelector<Control>('[name="ticket-close-target-search"]')?.focus());
    }),
  );
  lifetime.add(
    delegate(document.body, 'submit', TICKET_SELECTION_ACTIONS.submitTicketClose.selector, (event) => {
      event.preventDefault();
      void submitTicketClose();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_ACTIONS.cancelTicketClose.selector, () => {
      closeTicketCloseDialog();
    }),
  );
  lifetime.add(
    delegate(document.body, 'wa-hide', TICKET_SELECTION_TARGETS.ticketCloseDialog.selector, (event, target) => {
      if (event.composedPath()[0] === target) closeTicketCloseDialog();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_ACTIONS.openDuplicateTarget.selector, (_event, target) => {
      dependencies.ticketLinkReturnFocus = target as HTMLElement;
      void openDuplicateTarget(data(target).targetId ?? data(target).itemId!);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_ACTIONS.confirmBulkDelete.selector, () => {
      void executeBulkTicketAction({ kind: 'delete' }, dependencies.bulkTicketSlugs);
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'wa-hide',
      '[data-component="bulk-tag-dialog"], [data-component="bulk-delete-dialog"], [data-component="empty-trash-dialog"]',
      () => {
        bulkTicketDialog.value = undefined;
        dependencies.bulkTicketSlugs = [];
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'input', TICKET_SELECTION_TARGETS.notWorkingNoteField.selector, (_event, target) => {
      notWorkingNote.value = (target as HTMLTextAreaElement).value;
      scheduleProjectSessionPersistence();
      presentNotWorkingDialog();
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', 'input[name="not-working-attachments"]', (_event, target) => {
      const input = target as HTMLInputElement;
      if (input.files?.length) void addNotWorkingFiles(input.files);
      input.value = '';
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_ACTIONS.removeNotWorkingAttachment.selector, (_event, target) => {
      const id = data(target).pendingAttachmentId;
      if (id) void deleteDraftFiles(draftScope('not-working', notWorkingTarget.value.projectId), [id]);
      notWorkingFiles.value = notWorkingFiles.value.filter((item) => item.id !== id);
      scheduleProjectSessionPersistence();
    }),
  );
  lifetime.add(
    delegate(document.body, 'dragover', TICKET_SELECTION_TARGETS.notWorkingDropzone.selector, (event, target) => {
      event.preventDefault();
      (target as HTMLElement).dataset.dragging = 'true';
    }),
  );
  lifetime.add(
    delegate(document.body, 'dragleave', TICKET_SELECTION_TARGETS.notWorkingDropzone.selector, (_event, target) => {
      delete (target as HTMLElement).dataset.dragging;
    }),
  );
  lifetime.add(
    delegate(document.body, 'drop', TICKET_SELECTION_TARGETS.notWorkingDropzone.selector, (event, target) => {
      event.preventDefault();
      delete (target as HTMLElement).dataset.dragging;
      const files = (event as DragEvent).dataTransfer?.files;
      if (files?.length) void addNotWorkingFiles(files);
    }),
  );
  lifetime.add(
    delegate(document.body, 'submit', TICKET_SELECTION_ACTIONS.submitNotWorking.selector, (event) => {
      event.preventDefault();
      void submitNotWorking();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TICKET_SELECTION_ACTIONS.cancelNotWorking.selector, () => {
      closeNotWorking();
    }),
  );
  lifetime.add(
    delegate(document.body, 'wa-hide', TICKET_SELECTION_TARGETS.notWorkingDialog.selector, (_event, dialog) => {
      const target = notWorkingTarget.value,
        activeLabel = target.mode === 'reopen' ? `Reopen Ticket — ${target.slug}` : `Not Working — ${target.slug}`;
      if (!notWorkingSubmitting.value && target.slug && dialog.getAttribute('aria-label') === activeLabel)
        closeNotWorking();
    }),
  );
  lifetime.add(
    delegate(document.body, 'keydown', TICKET_SELECTION_ACTIONS.selectTicketRow.selector, (event, target) => {
      const keyboard = event as KeyboardEvent,
        row = target as HTMLElement,
        slug = data(row).ticketSlug!,
        ordered = selectionOrder(row);
      if (matchesShortcut('select-all-tickets', keyboard, keyboardShortcutOverrides.value, appleShortcutPlatform)) {
        event.preventDefault();
        const board = row.closest('[data-component="ticket-board"]'),
          selectable = board
            ? [...board.querySelectorAll<HTMLElement>('[data-action="select-ticket-row"]')].map(
                (ticket) => data(ticket).ticketSlug!,
              )
            : visibleTickets().map((ticket) => ticket.slug),
          next = selectAllTickets(selectable);
        dependencies.ticketSelectionAnchor = next.anchor;
        selectedTicketSlugs.value = [...next.selected];
        return;
      }
      if (keyboard.key === 'ArrowUp' || keyboard.key === 'ArrowDown') {
        event.preventDefault();
        const next = adjacentTicketSlug(ordered, slug, keyboard.key === 'ArrowDown' ? 1 : -1);
        if (next) {
          document.querySelector<HTMLElement>(`[data-ticket-slug="${next}"]`)?.focus();
          void selectTickets(next, { range: keyboard.shiftKey }, ordered);
        }
        return;
      }
      if (keyboard.key === 'Enter' || keyboard.key === ' ') {
        event.preventDefault();
        void selectTickets(slug, { range: keyboard.shiftKey, toggle: keyboard.metaKey || keyboard.ctrlKey }, ordered);
      }
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
