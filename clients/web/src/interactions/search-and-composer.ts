import { readSearchField } from '@kerfjs/ui/search-field';
import type { SearchFieldModel } from '@kerfjs/ui/search-field-model';
import { wireSearchFields } from '@kerfjs/ui/wire-search-fields';
import { attr, delegate, type Signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { type TicketRow as WireTicketRow } from '../api';
import { STRANDED_ATTACHMENTS_MESSAGE } from '../components/quick-ticket-composer';
import { wireQuickTicketComposer } from '../components/quick-ticket-composer-interactions';
import { wireSearchScopeToolbar } from '../components/search-scope-toolbar';
import { wireTicketRowUpNextToggle } from '../components/ticket-collection-interactions';
import {
  type WorkspaceSort,
  type WorkspaceSortDirection,
  type WorkspaceViewMode,
} from '../components/workspace-header';
import { wireWorkspaceHeaderActions } from '../components/workspace-header-interactions';
import { viewportSafeContextMenuPosition } from '../context-menu-position';
import { type InlineSearchToken } from '../inline-search';
import { restoreInlineSearchCaret } from '../inline-search-caret';
import { isWhitespaceOnlySearch, keepSearchOpenForShadowTarget } from '../search-collapse';
import { type BulkTicketAction } from '../ticket-bulk-operations';
import { saveLastTicketCategory } from '../ticket-category-preference';
import { type TicketHistory } from '../ticket-operations';
import { deleteDraftFiles } from '../workspace-session';
import { insertSearchSyntax, wireTicketSearchFields } from './ticket-search-field';
import { type PendingEvidence, type Project } from './types';

/** Live application bindings used by this handler group. */
export interface SearchAndComposerInteractionsDependencies {
  readonly searchOpen: Signal<boolean>;
  readonly guidanceSearchOpen: Signal<boolean>;
  readonly guidanceRecommendationsOpen: Signal<boolean>;
  readonly guidanceSearchModel: SearchFieldModel;
  readonly guidanceSearchActive: () => boolean;
  /** Kerf-managed ticket search models, keyed by their TicketSearchField ids (HS2-5JXBQY). */
  readonly workspaceSearchModel: SearchFieldModel;
  readonly savedViewSearchModel: SearchFieldModel;
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
  /** Switch the active search into All Tickets and back (HS2-4J6764). */
  readonly setAllTicketsSearch: (allTickets: boolean) => void;
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
  const lifetime = createScope();
  const {
    searchOpen,
    guidanceSearchOpen,
    guidanceSearchModel,
    guidanceSearchActive,
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
    setAllTicketsSearch,
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
      id === 'workspace-search'
        ? workspaceSearchModel
        : id === 'saved-view-query'
          ? savedViewSearchModel
          : id === 'guidance-search'
            ? guidanceSearchModel
            : undefined,
    focusField = (id: string, offset?: number) => {
      if (id === 'workspace-search') focusWorkspaceSearch(offset);
      else if (id === 'saved-view-query') focusSavedViewQuery(offset);
      else if (id === 'guidance-search')
        restoreInlineSearchCaret(document, '[data-search-field-editor="guidance-search"]', offset);
    },
    tokenOffset = (id: string, raw: string) =>
      id === 'workspace-search'
        ? workspaceSearchTokenOffset(raw)
        : id === 'saved-view-query'
          ? savedViewSearchTokenOffset(raw)
          : id === 'guidance-search'
            ? guidanceSearchModel.state.value.tokens.find((token) => token.value === raw)?.offset
            : undefined;
  lifetime.add(
    wireTicketSearchFields(document.body, {
      applyDate: (id, _prefix, value) => {
        modelFor(id)?.commit(value);
        focusField(id);
      },
      insertSyntax: (id, token) => {
        const model = modelFor(id);
        if (!model) return;
        insertSearchSyntax(model, token);
        if (id === 'workspace-search') searchHelpOpen.value = false;
        else if (id === 'saved-view-query') savedViewHelpOpen.value = false;
        requestAnimationFrame(() => {
          focusField(id);
        });
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
    }),
  );
  // Kerf owns the token-search editor chrome, collapsible reveal/focus/Escape/empty-blur, Enter
  // submit, and, through the registered models, parsing, chips, suggestions, chip edit/removal,
  // and clear (HS2-5JXBQY). Hot Sheet adopts its persisted workspace signal and retains the
  // caller-owned date/help surfaces.
  // An empty search blurred by a pointer press collapses only after that press's click, so the
  // collapsing row never shifts the control the user pressed (Kerf KF-64W0RN, HS2-YVBGW3).
  const tokenSearchFields = wireSearchFields(document.body, {
    models: {
      'workspace-search': workspaceSearchModel,
      'saved-view-query': savedViewSearchModel,
      'guidance-search': guidanceSearchModel,
    },
    collapsible: {
      signals: { 'workspace-search': searchOpen, 'guidance-search': guidanceSearchOpen },
      keepOpenOn: keepSearchOpenForShadowTarget,
    },
    // Enter commits a trailing filter through the model; the rebuilt editor gets its caret back at the end.
    onSubmit: ({ id }) => {
      focusField(id);
    },
  });
  lifetime.add(tokenSearchFields);
  const workspaceSearchEditor = attr('data-search-field-editor', 'workspace-search');
  // Kerf's empty-blur check counts whitespace as content. For the workspace's collapsible
  // field, whitespace has no search meaning, so close it once focus really leaves. Defer a
  // pointer blur until after click, just as Kerf does for a completely empty field.
  let pointerDown = false;
  let pendingWhitespaceBlur: (() => void) | undefined;
  let pointerReleaseTimer: ReturnType<typeof setTimeout> | undefined;
  const flushWhitespaceBlur = () => {
    if (pointerReleaseTimer !== undefined) clearTimeout(pointerReleaseTimer);
    pointerReleaseTimer = undefined;
    pointerDown = false;
    pendingWhitespaceBlur?.();
    pendingWhitespaceBlur = undefined;
  };
  const onPointerDown = () => {
    pointerDown = true;
  };
  const onPointerUp = () => {
    pointerReleaseTimer = setTimeout(flushWhitespaceBlur, 0);
  };
  document.addEventListener('pointerdown', onPointerDown, true);
  document.addEventListener('pointerup', onPointerUp, true);
  document.addEventListener('pointercancel', flushWhitespaceBlur, true);
  lifetime.add(() => {
    document.removeEventListener('pointerdown', onPointerDown, true);
    document.removeEventListener('pointerup', onPointerUp, true);
    document.removeEventListener('pointercancel', flushWhitespaceBlur, true);
    if (pointerReleaseTimer !== undefined) clearTimeout(pointerReleaseTimer);
  });
  lifetime.add(
    delegate(document.body, 'focusout', workspaceSearchEditor.selector, (event, target) => {
      const editor = target as HTMLElement;
      const field = editor.closest<HTMLElement>('[data-component="search-field"]');
      if (!field || field.dataset.collapsible !== 'true') return;
      const value = readSearchField(editor);
      if (!isWhitespaceOnlySearch(value)) return;
      const next = (event as FocusEvent).relatedTarget;
      if (next instanceof Element && (field.contains(next) || next.closest('[data-search-field-keep-open]'))) return;
      const collapse = () => {
        const current = document.querySelector<HTMLElement>(workspaceSearchEditor.selector);
        if (
          !current ||
          field.contains(document.activeElement) ||
          document.activeElement?.closest('[data-search-field-keep-open]')
        )
          return;
        const currentValue = readSearchField(current);
        if (isWhitespaceOnlySearch(currentValue)) {
          workspaceSearchModel.replace({ query: '', tokens: [] });
          searchOpen.value = false;
        }
      };
      if (pointerDown) pendingWhitespaceBlur = collapse;
      else queueMicrotask(collapse);
    }),
  );
  const applyWorkspaceSort = (next: { sort: WorkspaceSort; direction: WorkspaceSortDirection }) => {
    resetProgressiveTicketRendering();
    sort.value = next.sort;
    sortDirection.value = next.direction;
    persistWorkspacePreferences();
    if (searchQuery.value.trim() || searchTokens.value.length) scheduleTicketSearch();
    else void refreshProject({ showLoading: false });
  };
  lifetime.add(
    wireWorkspaceHeaderActions(document.body, {
      currentSort: () => ({ sort: sort.value, direction: sortDirection.value }),
      sort: applyWorkspaceSort,
      toggleSelectedUpNext: () => {
        const selected = selectedRows();
        if (selected.length > 0)
          void executeBulkTicketAction(
            { kind: 'up-next', value: !selected.every((ticket) => ticket.up_next) },
            selected.map((ticket) => ticket.slug),
          );
      },
      openSelectedTicketActions: (anchor) => {
        const selected = selectedRows();
        if (selected.length === 0) return;
        const rect = anchor.getBoundingClientRect(),
          position = viewportSafeContextMenuPosition(
            rect.right - 232,
            rect.bottom,
            window.innerWidth,
            window.innerHeight,
            { width: 232, height: 382 },
          );
        ticketContextMenu.value = { ...position, ticketSlug: selected[0].slug, hideUpNext: true };
      },
      openSearch: () => {
        tokenSearchFields.open(guidanceSearchActive() ? 'guidance-search' : 'workspace-search');
      },
      openGuidanceRecommendations: () => {
        if (guidanceSearchActive()) dependencies.guidanceRecommendationsOpen.value = true;
      },
      setViewMode: (mode) => {
        resetProgressiveTicketRendering();
        viewMode.value = mode;
        if (mode !== 'settings') dependencies.guidanceRecommendationsOpen.value = false;
        persistWorkspacePreferences();
        if (mode === 'list' || mode === 'board') void refreshProject({ showLoading: false });
      },
    }),
  );
  lifetime.add(wireSearchScopeToolbar(document.body, { setAllTickets: setAllTicketsSearch }));
  const resetComposerAttachmentMessage = () => {
    composerAttachmentMessage.value = '';
    composerAttachmentError.value = false;
  };
  lifetime.add(
    wireQuickTicketComposer(document.body, {
      expand: openTicketComposer,
      expanded: () => composerExpanded.value,
      canHide: () => !composerSubmitting.value,
      dismissed: resetTicketComposer,
      setTitle: (title) => {
        composerTitle.value = title;
        scheduleProjectSessionPersistence();
      },
      setDetails: (details) => {
        composerDetails.value = details;
        scheduleProjectSessionPersistence();
      },
      setCategory: (category) => {
        composerCategory.value = category;
        saveLastTicketCategory(localStorage, composerCategory.value);
        scheduleProjectSessionPersistence();
      },
      setSource: (source) => {
        // In-memory only: the pick lasts while the composer is open; creating remembers it (HS2-NZMJBJ).
        composerSource.value = source;
        // A blocked-submit explanation is about the previous source; the new one shows its own state.
        if (composerAttachmentMessage.value === STRANDED_ATTACHMENTS_MESSAGE) resetComposerAttachmentMessage();
      },
      toggleUpNext: () => {
        composerUpNext.value = !composerUpNext.value;
        scheduleProjectSessionPersistence();
      },
      addFiles: (files) => {
        void addNewTicketFiles(files);
      },
      removeAttachment: (id) => {
        if (id) void deleteDraftFiles(draftScope('composer'), [id]);
        composerAttachments.value = composerAttachments.value.filter((item) => item.id !== id);
        resetComposerAttachmentMessage();
        scheduleProjectSessionPersistence();
      },
      clearAttachments: () => {
        // Drops every file staged for a source that cannot take attachments (HS2-8HHHK3).
        const ids = composerAttachments.value.map((item) => item.id);
        if (ids.length) void deleteDraftFiles(draftScope('composer'), ids);
        composerAttachments.value = [];
        resetComposerAttachmentMessage();
        scheduleProjectSessionPersistence();
      },
      draggingTickets: () => Boolean(dependencies.draggedTickets),
      submit: () => {
        void submitNewTicket();
      },
    }),
  );
  lifetime.add(
    wireTicketRowUpNextToggle(document.body, (row) => {
      const ticket = tickets.value.find((item) => item.slug === row.dataset.ticketSlug);
      if (ticket) void history().execute(ticket.slug, { up_next: !ticket.up_next });
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
