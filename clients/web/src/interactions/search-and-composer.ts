import { wireTokenSearchFields } from '@kerfjs/ui/wire-token-search-fields';
import { batch, delegate, delegateCapture, type Signal } from 'kerfjs';

import { type TicketRow as WireTicketRow } from '../api';
import {
  nextWorkspaceSort,
  type WorkspaceSort,
  type WorkspaceSortDirection,
  type WorkspaceViewMode,
} from '../components/workspace-header';
import { viewportSafeContextMenuPosition } from '../context-menu-position';
import { activeDatePattern, type InlineSearchToken } from '../inline-search';
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
  readonly readWorkspaceSearchEditor: (editor: HTMLElement) => { text: string; tokens: InlineSearchToken[] };
  readonly updateTicketSearch: (
    value: string,
    forceToken?: boolean,
    currentTokens?: InlineSearchToken[],
    parseTokens?: boolean,
  ) => boolean;
  readonly restoreWorkspaceSearchEnd: () => void;
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
  readonly removeWorkspaceSearchToken: (raw: string) => boolean;
  readonly removeSavedViewQueryToken: (raw: string) => boolean;
  readonly addWorkspaceSearchTag: (tag: string) => void;
  readonly addSavedViewQueryTag: (tag: string) => void;
  readonly editWorkspaceSearchToken: (event: Event, target: Element) => void;
  readonly editSavedViewQueryToken: (event: Event, target: Element) => void;
  readonly searchHelpOpen: Signal<boolean>;
  readonly savedViewHelpOpen: Signal<boolean>;
  readonly savedViewQuery: Signal<string>;
  readonly savedViewError: Signal<string>;
  readonly replaceActiveWorkspaceSearchToken: (pattern: RegExp, token: InlineSearchToken) => void;
  readonly replaceActiveSavedViewQueryToken: (pattern: RegExp, token: InlineSearchToken) => void;
  readonly focusWorkspaceSearch: (offset?: number) => void;
  workspaceSearchEditingToken: boolean;
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
    readWorkspaceSearchEditor,
    updateTicketSearch,
    restoreWorkspaceSearchEnd,
    readInlineSearchField,
    savedViewQueryTokens,
    updateSavedViewQuery,
    focusSavedViewQuery,
    removeWorkspaceSearchToken,
    removeSavedViewQueryToken,
    addWorkspaceSearchTag,
    addSavedViewQueryTag,
    editWorkspaceSearchToken,
    editSavedViewQueryToken,
    searchHelpOpen,
    savedViewHelpOpen,
    savedViewQuery,
    savedViewError,
    replaceActiveWorkspaceSearchToken,
    replaceActiveSavedViewQueryToken,
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
    addNewTicketFiles,
    draftScope,
    composerAttachments,
    composerAttachmentMessage,
    composerAttachmentError,
    submitNewTicket,
    tickets,
    history,
  } = dependencies;
  // Kerf owns the token-search editor chrome, collapsible reveal/focus/Escape/empty-blur,
  // Enter submit, adjacent-chip keyboard, and caret restoration. Hot Sheet adopts its persisted
  // workspace signal and retains parsing plus the caller-owned suggestion/date/help surfaces.
  const tokenSearchFields = wireTokenSearchFields(document.body, {
    collapsible: { signals: { 'workspace-search': searchOpen } },
    onSubmit: ({ id, editor }) => {
      if (id === 'workspace-search') {
        const state = readWorkspaceSearchEditor(editor);
        if (updateTicketSearch(state.text, true, state.tokens)) restoreWorkspaceSearchEnd();
      } else if (id === 'saved-view-query') {
        const state = readInlineSearchField(editor, savedViewQueryTokens.value);
        if (updateSavedViewQuery(state.text, true, state.tokens)) focusSavedViewQuery();
      }
    },
    onEdit: ({ id, editor, event }) => {
      if (id !== 'workspace-search') return;
      const state = readWorkspaceSearchEditor(editor),
        commitsToken =
          (typeof event.data === 'string' && /\s$/.test(event.data)) ||
          (event.inputType === 'insertFromPaste' && /\s$/.test(state.text));
      if (updateTicketSearch(state.text, false, state.tokens, commitsToken)) restoreWorkspaceSearchEnd();
    },
    keyboard: {
      onRemoveToken: ({ id, value }) => {
        if (id === 'workspace-search') removeWorkspaceSearchToken(value);
        else if (id === 'saved-view-query') removeSavedViewQueryToken(value);
      },
    },
  });
  // One registration serves every TicketSearchField (workspace header, workspace-grid rail,
  // saved-view dialog); each callback routes by the owning field id (HS2-N5G6JS).
  wireTicketSearchFields(document.body, {
    selectTag: (id, tag) => {
      if (id === 'workspace-search') addWorkspaceSearchTag(tag);
      else if (id === 'saved-view-query') addSavedViewQueryTag(tag);
    },
    applyDate: (id, prefix, token) => {
      if (id === 'workspace-search') {
        replaceActiveWorkspaceSearchToken(activeDatePattern(prefix), token);
        focusWorkspaceSearch();
      } else if (id === 'saved-view-query') {
        replaceActiveSavedViewQueryToken(activeDatePattern(prefix), token);
        focusSavedViewQuery();
      }
    },
    toggleHelp: (id) => {
      if (id === 'workspace-search') searchHelpOpen.value = !searchHelpOpen.value;
      else if (id === 'saved-view-query') savedViewHelpOpen.value = !savedViewHelpOpen.value;
    },
    clear: (id) => {
      if (id === 'workspace-search') {
        dependencies.workspaceSearchEditingToken = false;
        batch(() => {
          searchQuery.value = '';
          searchTokens.value = [];
          searchHelpOpen.value = false;
        });
        scheduleTicketSearch();
      } else if (id === 'saved-view-query') {
        batch(() => {
          savedViewQuery.value = '';
          savedViewQueryTokens.value = [];
          savedViewError.value = '';
          savedViewHelpOpen.value = false;
        });
        focusSavedViewQuery(0);
      }
    },
    removeToken: (id, raw) => {
      if (id === 'workspace-search') removeWorkspaceSearchToken(raw);
      else if (id === 'saved-view-query') removeSavedViewQueryToken(raw);
    },
    editToken: (id, event, target) => {
      if (id === 'workspace-search') editWorkspaceSearchToken(event, target);
      else if (id === 'saved-view-query') editSavedViewQueryToken(event, target);
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
  delegate(document.body, 'wa-select', '.workspace-header__overflow', (event) => {
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
