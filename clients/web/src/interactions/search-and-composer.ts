import type { TokenSearchModel } from '@kerfjs/ui/token-search-model';
import { wireTokenSearchFields } from '@kerfjs/ui/wire-token-search-fields';
import { delegate, delegateCapture, type Signal } from 'kerfjs';

import { type TicketRow as WireTicketRow } from '../api';
import { STRANDED_ATTACHMENTS_MESSAGE } from '../components/quick-ticket-composer';
import {
  nextWorkspaceSort,
  type WorkspaceSort,
  type WorkspaceSortDirection,
  type WorkspaceViewMode,
} from '../components/workspace-header';
import { viewportSafeContextMenuPosition } from '../context-menu-position';
import { type InlineSearchToken } from '../inline-search';
import { type BulkTicketAction } from '../ticket-bulk-operations';
import { saveLastTicketCategory } from '../ticket-category-preference';
import { type TicketHistory } from '../ticket-operations';
import { deleteDraftFiles } from '../workspace-session';
import { data } from './dom';
import { wireTicketSearchFields } from './ticket-search-field';
import { type Control, type PendingEvidence, type Project } from './types';

/** Live application bindings used by this handler group. */
export interface SearchAndComposerInteractionsDependencies {
  readonly searchOpen: Signal<boolean>;
  /** Kerf-managed ticket search models, keyed by their TicketSearchField ids (HS2-5JXBQY). */
  readonly workspaceSearchModel: TokenSearchModel;
  readonly savedViewSearchModel: TokenSearchModel;
  /** A chip's caret position in each model, for focus restoration around Kerf's chip actions. */
  readonly workspaceSearchTokenOffset: (raw: string) => number | undefined;
  readonly savedViewSearchTokenOffset: (raw: string) => number | undefined;
  readonly focusSavedViewQuery: (offset?: number) => void;
  readonly searchHelpOpen: Signal<boolean>;
  readonly savedViewHelpOpen: Signal<boolean>;
  readonly focusWorkspaceSearch: (offset?: number) => void;
  readonly searchQuery: Signal<string>;
  readonly searchTokens: Signal<InlineSearchToken[]>;
  readonly scheduleTicketSearch: () => void;
  readonly refreshProject: (options?: { showLoading?: boolean }) => Promise<unknown>;
  readonly sort: { value: WorkspaceSort };
  readonly sortDirection: { value: WorkspaceSortDirection };
  readonly resetProgressiveTicketRendering: () => void;
  readonly persistWorkspacePreferences: () => void;
  readonly selectedRows: () => WireTicketRow[];
  readonly executeBulkTicketAction: (action: BulkTicketAction, targetSlugs?: string[]) => Promise<boolean>;
  readonly ticketContextMenu: Signal<{ x: number; y: number; ticketSlug: string; hideUpNext?: boolean } | undefined>;
  readonly viewMode: Signal<WorkspaceViewMode>;
  readonly openTicketComposer: (trigger?: HTMLElement) => void;
  readonly composerSubmitting: Signal<boolean>;
  readonly composerExpanded: Signal<boolean>;
  readonly resetTicketComposer: (clearStored?: boolean) => void;
  readonly composerTitle: Signal<string>;
  readonly scheduleProjectSessionPersistence: () => void;
  readonly composerDetails: Signal<string>;
  readonly composerCategory: Signal<string>;
  readonly composerUpNext: Signal<boolean>;
  readonly composerSource: Signal<string | undefined>;
  readonly addNewTicketFiles: (files: FileList | File[]) => Promise<void>;
  readonly draftScope: (kind: 'composer' | 'not-working', projectId?: string) => string;
  readonly composerAttachments: Signal<PendingEvidence[]>;
  readonly composerAttachmentMessage: Signal<string>;
  readonly composerAttachmentError: Signal<boolean>;
  draggedTickets: { slugs: string[]; source: Project } | undefined;
  readonly submitNewTicket: () => Promise<void>;
  readonly tickets: Signal<WireTicketRow[]>;
  readonly history: (projectId?: string) => TicketHistory;
}

/** Register this group only when the application wiring owner invokes it. */
export function wireSearchAndComposerInteractions(dependencies: SearchAndComposerInteractionsDependencies) {
  const {
    searchOpen,
    workspaceSearchModel,
    savedViewSearchModel,
    workspaceSearchTokenOffset,
    savedViewSearchTokenOffset,
    focusSavedViewQuery,
    searchHelpOpen,
    savedViewHelpOpen,
    focusWorkspaceSearch,
    searchQuery,
    searchTokens,
    scheduleTicketSearch,
    refreshProject,
    sort,
    sortDirection,
    resetProgressiveTicketRendering,
    persistWorkspacePreferences,
    selectedRows,
    executeBulkTicketAction,
    ticketContextMenu,
    viewMode,
    openTicketComposer,
    composerSubmitting,
    composerExpanded,
    resetTicketComposer,
    composerTitle,
    scheduleProjectSessionPersistence,
    composerDetails,
    composerCategory,
    composerUpNext,
    composerSource,
    addNewTicketFiles,
    draftScope,
    composerAttachments,
    composerAttachmentMessage,
    composerAttachmentError,
    submitNewTicket,
    tickets,
    history,
  } = dependencies;
  // One registration serves every TicketSearchField (workspace header, workspace-grid rail,
  // saved-view dialog); each callback routes by the owning field id (HS2-N5G6JS). It is wired
  // before Kerf's so the focus handlers read a chip's position before Kerf removes or expands it.
  const modelFor = (id: string) =>
      id === 'workspace-search' ? workspaceSearchModel : id === 'saved-view-query' ? savedViewSearchModel : undefined,
    focusField = (id: string, offset?: number) => {
      if (id === 'workspace-search') focusWorkspaceSearch(offset);
      else if (id === 'saved-view-query') focusSavedViewQuery(offset);
    },
    tokenOffset = (id: string, raw: string) =>
      id === 'workspace-search'
        ? workspaceSearchTokenOffset(raw)
        : id === 'saved-view-query'
          ? savedViewSearchTokenOffset(raw)
          : undefined;
  wireTicketSearchFields(document.body, {
    applyDate: (id, _prefix, value) => {
      modelFor(id)?.commit(value);
      focusField(id);
    },
    toggleHelp: (id) => {
      if (id === 'workspace-search') searchHelpOpen.value = !searchHelpOpen.value;
      else if (id === 'saved-view-query') savedViewHelpOpen.value = !savedViewHelpOpen.value;
    },
    clear: (id) => {
      if (id === 'workspace-search') searchHelpOpen.value = false;
      else if (id === 'saved-view-query') savedViewHelpOpen.value = false;
    },
    // Kerf removes or expands the chip in its own handler and re-renders the editor afterwards; the
    // caret goes back once that frame has painted the rebuilt editor.
    removeToken: (id, raw) => {
      const offset = tokenOffset(id, raw);
      requestAnimationFrame(() => {
        focusField(id, offset);
      });
    },
    editToken: (id, raw) => {
      const offset = tokenOffset(id, raw);
      requestAnimationFrame(() => {
        focusField(id, offset === undefined ? undefined : offset + raw.length);
      });
    },
  });
  // Kerf owns the token-search editor chrome, collapsible reveal/focus/Escape/empty-blur, Enter
  // submit, and, through the registered models, parsing, chips, suggestions, chip edit/removal,
  // and clear (HS2-5JXBQY). Hot Sheet adopts its persisted workspace signal and retains the
  // caller-owned date/help surfaces.
  // An empty search blurred by a pointer press collapses only after that press's click, so the
  // collapsing row never shifts the control the user pressed (Kerf KF-64W0RN, HS2-YVBGW3).
  const tokenSearchFields = wireTokenSearchFields(document.body, {
    models: { 'workspace-search': workspaceSearchModel, 'saved-view-query': savedViewSearchModel },
    collapsible: { signals: { 'workspace-search': searchOpen } },
    // Enter commits a trailing filter through the model; the rebuilt editor gets its caret back at the end.
    onSubmit: ({ id }) => {
      focusField(id);
    },
  });
  delegate(document.body, 'click', 'wa-select[name="workspace-sort"] wa-option', (_event, target) => {
    const next = nextWorkspaceSort(sort.value, sortDirection.value, (target as Control).value as WorkspaceSort);
    resetProgressiveTicketRendering();
    sort.value = next.sort;
    sortDirection.value = next.direction;
    persistWorkspacePreferences();
    if (searchQuery.value.trim() || searchTokens.value.length) scheduleTicketSearch();
    else void refreshProject({ showLoading: false });
  });
  delegate(document.body, 'wa-select', '[data-workspace-overflow]', (event) => {
    const item = (event as CustomEvent<{ item: HTMLElement }>).detail.item,
      action = item.dataset.workspaceOverflowAction;
    if (action === 'toggle-selected-up-next') {
      const selected = selectedRows();
      if (selected.length > 0)
        void executeBulkTicketAction(
          { kind: 'up-next', value: !selected.every((ticket) => ticket.up_next) },
          selected.map((ticket) => ticket.slug),
        );
      return;
    }
    if (action === 'open-selected-ticket-actions') {
      const selected = selectedRows();
      if (selected.length === 0) return;
      const rect = item.getBoundingClientRect(),
        position = viewportSafeContextMenuPosition(
          rect.right - 232,
          rect.bottom,
          window.innerWidth,
          window.innerHeight,
          { width: 232, height: 382 },
        );
      ticketContextMenu.value = { ...position, ticketSlug: selected[0].slug, hideUpNext: true };
      return;
    }
    if (action === 'open-workspace-search') {
      tokenSearchFields.open('workspace-search');
      return;
    }
    if (action === 'set-view-mode') {
      const mode = item.dataset.viewMode as WorkspaceViewMode;
      resetProgressiveTicketRendering();
      viewMode.value = mode;
      persistWorkspacePreferences();
      if (mode === 'list' || mode === 'board') void refreshProject({ showLoading: false });
      return;
    }
    if (action === 'set-workspace-sort') {
      const selected = item.dataset.workspaceSort as WorkspaceSort | undefined;
      if (!selected) return;
      const next = nextWorkspaceSort(sort.value, sortDirection.value, selected);
      resetProgressiveTicketRendering();
      sort.value = next.sort;
      sortDirection.value = next.direction;
      persistWorkspacePreferences();
      if (searchQuery.value.trim() || searchTokens.value.length) scheduleTicketSearch();
      else void refreshProject({ showLoading: false });
    }
  });
  delegate(document.body, 'click', '[data-action="expand-ticket-composer"]', (_event, target) => {
    openTicketComposer(target as HTMLElement);
  });
  delegateCapture(document.body, 'wa-hide', '[data-component="quick-ticket-composer"]', (event, target) => {
    if (event.target === target && composerSubmitting.value) event.preventDefault();
  });
  delegateCapture(document.body, 'wa-after-hide', '[data-component="quick-ticket-composer"]', (event, target) => {
    if (event.target === target && composerExpanded.value) resetTicketComposer();
  });
  delegate(document.body, 'input', '[name="new-ticket-title"]', (_event, target) => {
    composerTitle.value = (target as Control).value;
    scheduleProjectSessionPersistence();
  });
  delegate(document.body, 'input', '[name="new-ticket-details"]', (_event, target) => {
    composerDetails.value = (target as HTMLTextAreaElement).value;
    scheduleProjectSessionPersistence();
  });
  delegate(document.body, 'change', '[name="new-ticket-category"]', (_event, target) => {
    composerCategory.value = (target as Control).value;
    saveLastTicketCategory(localStorage, composerCategory.value);
    scheduleProjectSessionPersistence();
  });
  delegate(document.body, 'change', '[name="new-ticket-source"]', (_event, target) => {
    // In-memory only: the pick lasts while the composer is open; creating remembers it (HS2-NZMJBJ).
    composerSource.value = (target as Control).value || undefined;
    // A blocked-submit explanation is about the previous source; the new one shows its own state.
    if (composerAttachmentMessage.value === STRANDED_ATTACHMENTS_MESSAGE) {
      composerAttachmentMessage.value = '';
      composerAttachmentError.value = false;
    }
  });
  delegate(document.body, 'click', '[data-action="toggle-new-ticket-up-next"]', () => {
    composerUpNext.value = !composerUpNext.value;
    scheduleProjectSessionPersistence();
  });
  delegate(document.body, 'change', 'input[name="new-ticket-attachments"]', (_event, target) => {
    const input = target as HTMLInputElement;
    if (input.files?.length) void addNewTicketFiles(input.files);
    input.value = '';
  });
  delegate(document.body, 'click', '[data-action="remove-new-ticket-attachment"]', (_event, target) => {
    const id = data(target).pendingAttachmentId;
    if (id) void deleteDraftFiles(draftScope('composer'), [id]);
    composerAttachments.value = composerAttachments.value.filter((item) => item.id !== id);
    composerAttachmentMessage.value = '';
    composerAttachmentError.value = false;
    scheduleProjectSessionPersistence();
  });
  delegate(document.body, 'click', '[data-action="clear-new-ticket-attachments"]', () => {
    // Drops every file staged for a source that cannot take attachments (HS2-8HHHK3).
    const ids = composerAttachments.value.map((item) => item.id);
    if (ids.length) void deleteDraftFiles(draftScope('composer'), ids);
    composerAttachments.value = [];
    composerAttachmentMessage.value = '';
    composerAttachmentError.value = false;
    scheduleProjectSessionPersistence();
  });
  delegate(document.body, 'dragover', '[data-new-ticket-drop-target="true"]', (event, target) => {
    if (dependencies.draggedTickets) return;
    event.preventDefault();
    (target as HTMLElement).dataset.dragging = 'true';
  });
  delegate(document.body, 'dragleave', '[data-new-ticket-drop-target="true"]', (_event, target) => {
    delete (target as HTMLElement).dataset.dragging;
  });
  delegate(document.body, 'drop', '[data-new-ticket-drop-target="true"]', (event, target) => {
    if (dependencies.draggedTickets) return;
    event.preventDefault();
    event.stopPropagation();
    delete (target as HTMLElement).dataset.dragging;
    const files = (event as DragEvent).dataTransfer?.files;
    if (files?.length) {
      if (!composerExpanded.value) openTicketComposer(target as HTMLElement);
      void addNewTicketFiles(files);
    }
  });
  delegate(document.body, 'submit', '[data-action="create-ticket-form"]', (event) => {
    event.preventDefault();
    void submitNewTicket();
  });
  delegate(document.body, 'click', '[data-action="toggle-row-up-next"]', (event, target) => {
    event.stopPropagation();
    const article = target.closest('[data-ticket-slug]') as HTMLElement,
      ticket = tickets.value.find((item) => item.slug === article.dataset.ticketSlug);
    if (ticket) void history().execute(ticket.slug, { up_next: !ticket.up_next });
  });
}
