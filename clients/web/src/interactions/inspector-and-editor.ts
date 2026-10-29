import { delegate, delegateCapture, type Signal } from 'kerfjs';
import { type OverlayHandle } from 'kerfjs/overlay';
import { createScope } from 'kerfjs/scope';

import { Api, type CodeReview, type FullTicket } from '../api';
import { boardColumnStatus, isPerColumnBoardView } from '../board-pagination';
import { codeReviewTarget } from '../components/ticket-code-review';
import {
  wireMarkdownDetailsEditor,
  wireNoteComposer,
  wireNoteEditor,
} from '../components/ticket-content-editor-interactions';
import {
  type FeedbackChoiceModifiers,
  wireBlockedReasonEditor,
  wireFeedbackChoices,
  wireTicketTagEditor,
  wireTicketTitleEditor,
} from '../components/ticket-field-editor-interactions';
import { type InspectorTab } from '../components/ticket-inspector';
import { type TicketReaderDialogElement } from '../components/ticket-reader';
import { statusMenuAnchorY } from '../components/ticket-status-menu';
import { addTicketTag, openTicketTagSurface, removeTicketTag } from '../components/ticket-tag-editor';
import { type WorkspaceViewMode } from '../components/workspace-header';
import { maintainContextPopupMenuAnchor } from '../context-menu-position';
import { copyText } from '../copy-text';
import { createCoalescedInput, createKeyedCoalescedInput, type DebouncedAutosave } from '../debounced-autosave';
import { coalescedDraft, coalescedKeyedDraft, flushDraftInputs } from '../draft-input-coalescing';
import { parseFeedbackChoices, updateFeedbackChoiceDocumentSelection } from '../feedback-choices';
import { DETAILS_FEEDBACK_ID } from '../feedback-needed';
import { beginFeedbackPromptEdit, endFeedbackPromptEdit, isFeedbackPromptEdit } from '../feedback-prompt-edits';
import { combineFeedbackReply, type InlineFeedbackReply, sourceOffsetForVisibleOffset } from '../feedback-replies';
import type { FeedbackResponseDrafts, FeedbackResponseOwner } from '../feedback-response-drafts';
import { HostOnlyActionError } from '../host-only-actions';
import { inlineEditorEscape } from '../inline-editor-escape';
import { ATTACHMENTS_AND_GALLERY_TARGETS } from '../interaction-attrs/attachments-and-gallery';
import { INSPECTOR_AND_EDITOR_ACTIONS, INSPECTOR_AND_EDITOR_TARGETS } from '../interaction-attrs/inspector-and-editor';
import { isPopupMenuOpen } from '../popup-menu-state';
import { manuallyResizedTicketEditorHeight, saveTicketEditorSize, ticketEditorKind } from '../ticket-editor-size';
import { noteEditPatch, type TicketFieldConflict } from '../ticket-field-reconciliation';
import { type TicketPatch } from '../ticket-operations';
import {
  clearFrameFeedbackReply,
  reconcileTicketReaderFrame,
  startedPhasePatch,
  ticketEditTarget,
  type TicketReaderFrame,
} from '../ticket-reader-stack';
import { toggleTicketTitleExpanded } from '../ticket-title-disclosure';
import { type TicketTitleEditSurface, ticketTitleEditSurfaceOf } from '../ticket-title-editing';
import { type TicketView } from '../ticket-views';
import { data } from './dom';
import { type Control, type DetailsFinishTask, type Project } from './types';

/** Live application bindings used by this handler group. */
export interface InspectorAndEditorInteractionsDependencies {
  readonly selectedTicket: Signal<FullTicket | null>;
  readonly updateSelectedTracked: (patch: TicketPatch) => Promise<boolean>;
  readonly showToast: (message: string) => void;
  readonly error: Signal<string>;
  readonly readerOpen: Signal<boolean>;
  readonly readerDetailsDraft: Signal<string>;
  readerDetailsDraftBase: string;
  readonly detailsDraft: Signal<string>;
  detailsDraftBase: string;
  readonly titleDraft: Signal<string>;
  titleDraftBase: string;
  readonly readerBlockedReasonDraft: Signal<string>;
  readerBlockedReasonDraftBase: string;
  readonly blockedReasonDraft: Signal<string>;
  blockedReasonDraftBase: string;
  readonly readerNoteDraft: Signal<string>;
  readerNoteDraftBase: string;
  readonly noteDraft: Signal<string>;
  noteDraftBase: string;
  readonly fieldConflictResolution: Signal<string>;
  readonly fieldConflict: Signal<TicketFieldConflict | undefined>;
  readonly canUpdateSelected: () => boolean;
  readonly canEditStartedPhaseSelected: () => boolean;
  /** Whether a ticket of `connectionId` in project `projectId` records git started phases (HS2-ANRXAE). */
  readonly canEditStartedPhaseFor: (projectId: string, connectionId: string) => boolean;
  /** The one surface whose title editor is open (HS2-2M5BBN). */
  readonly titleEditingSurface: Signal<TicketTitleEditSurface | undefined>;
  readonly activeTicketSurface: () => ParentNode;
  readonly titleAutosave: DebouncedAutosave<string>;
  readonly tagsAutosave: DebouncedAutosave<string[]>;
  pointerDetailsReader: boolean | undefined;
  pointerDetailsFinish: DetailsFinishTask | undefined;
  readonly beginDetailsEdit: (reader?: boolean, frame?: TicketReaderFrame) => void;
  readonly linkedReaderFrame: (target: Element) => TicketReaderFrame | undefined;
  readonly replaceLinkedReaderFrame: (id: string, update: (frame: TicketReaderFrame) => TicketReaderFrame) => void;
  readonly linkedReaderSaves: (id: string) => {
    details: DebouncedAutosave<string>;
    note: DebouncedAutosave<{ id: string; value: string }>;
    blocked: DebouncedAutosave<string>;
    /** Answer a feedback request in the frame's ticket with a new regular note. */
    respond: (value: string, requestId?: string) => Promise<boolean>;
  };
  readonly readerDetailsAutosave: DebouncedAutosave<string>;
  readonly detailsAutosave: DebouncedAutosave<string>;
  /** Local recovery copy of a field's unsaved edit, rebased onto its current value (HS2-RE1PS6). */
  readonly restoreTicketDraft: (
    field: 'details' | 'title' | 'blocked_reason' | 'note',
    current: string,
    noteId?: string,
  ) => { draft: string; base: string } | undefined;
  readonly beginDetailsFinish: (reader?: boolean) => DetailsFinishTask;
  readonly finishDetailsEdit: (reader?: boolean) => Promise<boolean>;
  readonly readerEditingNoteId: Signal<string | undefined>;
  readonly editingNoteId: Signal<string | undefined>;
  readonly canAddNotes: () => boolean;
  readonly composingNote: Signal<boolean>;
  readonly newNoteDraft: Signal<string>;
  readonly scheduleProjectSessionPersistence: () => void;
  readonly updateSelected: (patch: Record<string, unknown>) => Promise<boolean>;
  readonly feedbackResponseDrafts: FeedbackResponseDrafts;
  readonly dismissFeedbackRequest: (requestId: string, frameId?: string) => Promise<boolean>;
  readonly readerNoteAutosave: DebouncedAutosave<{ id: string; value: string }>;
  readonly noteAutosave: DebouncedAutosave<{ id: string; value: string }>;
  readonly readerInlineFeedbackReplies: Signal<Record<string, InlineFeedbackReply[]>>;
  readonly readerFeedbackChoiceSelections: Signal<Record<string, string[]>>;
  readonly readerFeedbackChoiceAnchors: Map<string, string>;
  readonly project: () => Project | undefined;
  readonly canDeleteNotes: () => boolean;
  readonly api: () => Api;
  readonly refreshProject: ({ showLoading }?: { showLoading?: boolean }) => Promise<void>;
  readonly viewMode: Signal<WorkspaceViewMode>;
  readonly selectedView: Signal<TicketView>;
  readonly workspaceSearchActive: () => boolean;
  readonly loadBoardColumnMore: (columnId: string) => Promise<void>;
  readonly loadNextTicketPage: () => Promise<void>;
  readonly readerBlockedReasonEditing: Signal<boolean>;
  readonly blockedReasonEditing: Signal<boolean>;
  readonly readerBlockedReasonAutosave: DebouncedAutosave<string>;
  readonly blockedReasonAutosave: DebouncedAutosave<string>;
  readonly presentTicketReaderDialog: (
    id: string,
    trigger: HTMLElement | undefined,
    onOpen: () => void,
    afterOpen?: () => void,
  ) => void;
  readonly readerTab: Signal<InspectorTab>;
  readonly codeReviewLoading: Signal<boolean>;
  readonly refreshCodeReview: () => Promise<void>;
  readonly refreshTicketUsage: () => Promise<void>;
  readonly readerDialog: (id: string) => TicketReaderDialogElement | null;
  readonly approveTicketReaderClose: (dialog: TicketReaderDialogElement) => void;
  readonly finishTicketReaderClose: (dialog: TicketReaderDialogElement) => void;
  readonly readerLargeText: Signal<boolean>;
  readonly linkedReaderStack: Signal<TicketReaderFrame[]>;
  readonly inspectorTab: Signal<InspectorTab>;
  readonly codeReviewMessage: Signal<string>;
  readonly codeReview: Signal<CodeReview | undefined>;
}

/** Register this group only when the application wiring owner invokes it. */
export function wireInspectorAndEditorInteractions(dependencies: InspectorAndEditorInteractionsDependencies) {
  const lifetime = createScope();
  const {
    selectedTicket,
    updateSelectedTracked,
    showToast,
    error,
    readerOpen,
    readerDetailsDraft,
    detailsDraft,
    titleDraft,
    readerBlockedReasonDraft,
    blockedReasonDraft,
    readerNoteDraft,
    noteDraft,
    fieldConflictResolution,
    fieldConflict,
    canUpdateSelected,
    canEditStartedPhaseSelected,
    canEditStartedPhaseFor,
    titleEditingSurface,
    activeTicketSurface,
    titleAutosave,
    tagsAutosave,
    beginDetailsEdit,
    linkedReaderFrame,
    replaceLinkedReaderFrame,
    linkedReaderSaves,
    readerDetailsAutosave,
    restoreTicketDraft,
    detailsAutosave,
    beginDetailsFinish,
    finishDetailsEdit,
    readerEditingNoteId,
    editingNoteId,
    canAddNotes,
    composingNote,
    newNoteDraft,
    scheduleProjectSessionPersistence,
    updateSelected,
    dismissFeedbackRequest,
    feedbackResponseDrafts,
    readerNoteAutosave,
    noteAutosave,
    readerInlineFeedbackReplies,
    readerFeedbackChoiceSelections,
    readerFeedbackChoiceAnchors,
    project,
    canDeleteNotes,
    api,
    refreshProject,
    viewMode,
    selectedView,
    workspaceSearchActive,
    loadBoardColumnMore,
    loadNextTicketPage,
    readerBlockedReasonEditing,
    blockedReasonEditing,
    readerBlockedReasonAutosave,
    blockedReasonAutosave,
    presentTicketReaderDialog,
    readerTab,
    codeReviewLoading,
    refreshCodeReview,
    refreshTicketUsage,
    readerDialog,
    approveTicketReaderClose,
    finishTicketReaderClose,
    readerLargeText,
    linkedReaderStack,
    inspectorTab,
    codeReviewMessage,
    codeReview,
  } = dependencies;
  // Field edits target the top linked ticket reader when one is open, exactly like the workspace
  // reader otherwise (HS2-KRVWKF): updateSelected/updateSelectedTracked already route patches there,
  // so the ticket the control reads, its provider capability, and its optimistic value follow suit.
  const editTarget = () => ticketEditTarget(linkedReaderStack.value, selectedTicket.value, canUpdateSelected);
  const editTicket = (): FullTicket | null => editTarget().ticket;
  const canUpdateEditTicket = () => editTarget().canUpdate;
  function setEditTicket(ticket: FullTicket) {
    const linked = editTarget().frame;
    if (linked) replaceLinkedReaderFrame(linked.id, (frame) => ({ ...frame, ticket }));
    else selectedTicket.value = ticket;
  }
  const conflictResolutionInput = createCoalescedInput<string>((value) => {
    fieldConflictResolution.value = value;
  });
  lifetime.add(() => {
    conflictResolutionInput.cancel();
  });
  let stopStatusMenuAnchor: (() => void) | undefined;
  lifetime.add(() => stopStatusMenuAnchor?.());
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.toggleInspectorUpNext.selector, () => {
      const ticket = editTicket();
      if (ticket && canUpdateEditTicket()) void updateSelectedTracked({ up_next: !ticket.up_next });
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.copyTicketSlug.selector, (_event, target) => {
      const slug = target.closest<HTMLElement>('[data-ticket-slug]')?.dataset.ticketSlug;
      if (!slug) return;
      void copyText(slug)
        .then(() => {
          showToast(`${slug} copied to clipboard.`);
        })
        .catch((reason: unknown) => {
          error.value = `Copy failed: ${reason instanceof Error ? reason.message : String(reason)}`;
        });
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'change',
      INSPECTOR_AND_EDITOR_TARGETS.inspectorCategoryField.selector,
      (_event, target) => {
        void updateSelectedTracked({ category: (target as Control).value });
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'change',
      INSPECTOR_AND_EDITOR_TARGETS.inspectorPriorityField.selector,
      (_event, target) => {
        void updateSelectedTracked({ priority: (target as Control).value });
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.openInspectorStatusMenu.selector, (event, target) => {
      if (!(event.target instanceof Element) || !event.target.closest('[data-inspector-status-menu] > button')) return;
      const root = target.closest('#app-right-rail, [data-component="ticket-reader"]');
      const menu = root?.querySelector('[data-inspector-status-menu]');
      if (!root || !menu || !isPopupMenuOpen(menu)) return;
      const rect = (target.querySelector('[data-inspector-status-menu] > button') ?? target).getBoundingClientRect();
      const ticketId = selectedTicket.peek()?.id;
      stopStatusMenuAnchor?.();
      stopStatusMenuAnchor = maintainContextPopupMenuAnchor(
        '[data-inspector-status-menu]',
        { x: rect.left, y: statusMenuAnchorY(rect.bottom, window.innerWidth, window.innerHeight) },
        () => root.isConnected && selectedTicket.peek()?.id === ticketId,
        root,
      );
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.setInspectorStatus.selector, (_event, target) => {
      if (!canUpdateEditTicket()) return;
      const status = (target as HTMLElement).dataset.ticketStatus;
      if (status) {
        stopStatusMenuAnchor?.();
        stopStatusMenuAnchor = undefined;
        void updateSelectedTracked({ status });
      }
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      INSPECTOR_AND_EDITOR_ACTIONS.setInspectorStartedPhase.selector,
      (_event, target) => {
        // A linked reader frame records its own ticket's phase through its own project (HS2-ANRXAE).
        const frame = linkedReaderFrame(target);
        if (
          frame
            ? !frame.capabilities.update || !canEditStartedPhaseFor(frame.projectId, frame.ticket.connection_id)
            : !canEditStartedPhaseSelected()
        )
          return;
        const phase = (target as HTMLElement).dataset.startedPhase;
        if (phase === undefined) return;
        stopStatusMenuAnchor?.();
        stopStatusMenuAnchor = undefined;
        void updateSelectedTracked(startedPhasePatch(frame?.ticket ?? selectedTicket.value, phase));
      },
    ),
  );
  // Keystrokes reach the draft signals through typing-pause coalescers (HS2-EQTXTJ).
  const titleDraftInput = coalescedDraft<string>((value) => {
      titleDraft.value = value;
    }),
    detailsDraftInput = coalescedDraft<{ reader: boolean; value: string }>(({ reader, value }) => {
      (reader ? readerDetailsDraft : detailsDraft).value = value;
    }),
    noteDraftInput = coalescedDraft<{ reader: boolean; value: string; responseOwner?: FeedbackResponseOwner }>(
      ({ reader, value, responseOwner }) => {
        if (responseOwner && !feedbackResponseDrafts.acceptsInput(responseOwner)) return;
        (reader ? readerNoteDraft : noteDraft).value = value;
      },
    ),
    newNoteDraftInput = coalescedDraft<string>((value) => {
      newNoteDraft.value = value;
      scheduleProjectSessionPersistence();
    }),
    blockedReasonDraftInput = coalescedDraft<{ reader: boolean; value: string }>(({ reader, value }) => {
      (reader ? readerBlockedReasonDraft : blockedReasonDraft).value = value;
    }),
    // Linked reader frames keep their drafts in the frame; replacing it re-renders the workspace,
    // so it too catches up at typing pauses, per frame (and note) (HS2-WV0VPE).
    linkedDetailsDraftInput = coalescedKeyedDraft<{ frameId: string; value: string }>(
      ({ frameId }) => frameId,
      ({ frameId, value }) => {
        replaceLinkedReaderFrame(frameId, (current) => ({
          ...current,
          edit: { ...current.edit, detailsDraft: value },
        }));
      },
    ),
    linkedNoteDraftInput = coalescedKeyedDraft<{
      frameId: string;
      id: string;
      value: string;
      responseOwner?: FeedbackResponseOwner;
    }>(
      ({ frameId, id }) => `${frameId}\u0000${id}`,
      ({ frameId, id, value, responseOwner }) => {
        if (responseOwner && !feedbackResponseDrafts.acceptsInput(responseOwner)) return;
        replaceLinkedReaderFrame(frameId, (current) => ({
          ...current,
          edit: {
            ...current.edit,
            editingNoteId: id,
            noteDraft: value,
            ...(responseOwner ? { responseRequestId: id } : {}),
          },
        }));
      },
    ),
    linkedBlockedReasonDraftInput = coalescedKeyedDraft<{ frameId: string; value: string }>(
      ({ frameId }) => frameId,
      ({ frameId, value }) => {
        replaceLinkedReaderFrame(frameId, (current) => ({
          ...current,
          edit: { ...current.edit, blockedReasonDraft: value },
        }));
      },
    ),
    linkedNewNoteDraftInput = coalescedKeyedDraft<{ frameId: string; value: string }>(
      ({ frameId }) => frameId,
      ({ frameId, value }) => {
        replaceLinkedReaderFrame(frameId, (current) => ({
          ...current,
          edit: { ...current.edit, newNoteDraft: value },
        }));
      },
    );
  function setFrameComposer(frameId: string, composing: boolean, draft: string) {
    replaceLinkedReaderFrame(frameId, (current) => ({
      ...current,
      edit: {
        ...current.edit,
        ...(composing ? { editingNoteId: undefined } : {}),
        composingNote: composing,
        newNoteDraft: draft,
      },
    }));
  }
  lifetime.add(() => {
    flushDraftInputs();
  });
  function updateConflictDraft(field: string, value: string, base = value) {
    flushDraftInputs();
    if (field === 'details' && readerOpen.value) {
      readerDetailsDraft.value = value;
      dependencies.readerDetailsDraftBase = base;
    } else if (field === 'details') {
      detailsDraft.value = value;
      dependencies.detailsDraftBase = base;
    } else if (field === 'title') {
      titleDraft.value = value;
      dependencies.titleDraftBase = base;
    } else if (field === 'blocked_reason' && readerOpen.value) {
      readerBlockedReasonDraft.value = value;
      dependencies.readerBlockedReasonDraftBase = base;
    } else if (field === 'blocked_reason') {
      blockedReasonDraft.value = value;
      dependencies.blockedReasonDraftBase = base;
    } else if (field === 'note' && readerOpen.value) {
      readerNoteDraft.value = value;
      dependencies.readerNoteDraftBase = base;
    } else if (field === 'note') {
      noteDraft.value = value;
      dependencies.noteDraftBase = base;
    }
  }
  function isReaderSurface(target: Element) {
    return Boolean(target.closest('[data-component="ticket-reader"]'));
  }
  function resolvedConflictPatch(conflict: TicketFieldConflict, value: string): TicketPatch {
    if (conflict.field === 'note')
      return noteEditPatch(selectedTicket.value, conflict.key.slice('note:'.length), value);
    if (conflict.field === 'tags')
      return {
        tags: value
          .split(',')
          .map((item) => item.trim())
          .filter(Boolean),
      };
    if (conflict.field === 'up_next') return { up_next: value.toLocaleLowerCase() === 'true' };
    if (conflict.field === 'blocked_reason') return { blocked_reason: value.trim() || null };
    return { [conflict.field]: value };
  }
  lifetime.add(
    delegate(
      document.body,
      'input',
      INSPECTOR_AND_EDITOR_TARGETS.ticketConflictResolutionField.selector,
      (_event, target) => {
        // The textarea keeps the live merge draft; the inspector re-renders per typing pause (HS2-9CWDSB).
        conflictResolutionInput.schedule((target as HTMLTextAreaElement).value);
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'focusout', INSPECTOR_AND_EDITOR_TARGETS.ticketConflictResolutionField.selector, () => {
      conflictResolutionInput.flush();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.acceptRemoteTicketField.selector, () => {
      conflictResolutionInput.cancel();
      const conflict = fieldConflict.value;
      if (!conflict) return;
      updateConflictDraft(conflict.field, conflict.theirs);
      fieldConflict.value = undefined;
      fieldConflictResolution.value = '';
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.applyTicketFieldMerge.selector, () => {
      conflictResolutionInput.flush();
      const conflict = fieldConflict.value;
      if (!conflict) return;
      const value = fieldConflictResolution.value;
      if (conflict.field === 'title' && !value.trim()) {
        error.value = 'Ticket title cannot be empty.';
        return;
      }
      updateConflictDraft(conflict.field, value, conflict.theirs);
      fieldConflict.value = undefined;
      fieldConflictResolution.value = '';
      void updateSelectedTracked(resolvedConflictPatch(conflict, value));
    }),
  );
  function beginTitleEdit(trigger: Element) {
    const ticket = editTicket();
    if (!ticket || !canUpdateEditTicket()) return;
    const restoredTitle = restoreTicketDraft('title', ticket.title);
    titleDraft.value = restoredTitle?.draft ?? ticket.title;
    dependencies.titleDraftBase = restoredTitle?.base ?? ticket.title;
    // Only the surface that started the edit shows the editor; the other keeps its heading (HS2-2M5BBN).
    titleEditingSurface.value = ticketTitleEditSurfaceOf(trigger);
    if (restoredTitle) titleAutosave.schedule(titleDraft.value);
    queueMicrotask(() => activeTicketSurface().querySelector<HTMLElement>('[name="ticket-title"]')?.focus());
  }
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.toggleTicketTitle.selector, (_event, target) => {
      const disclosure = target.closest<HTMLElement>('[data-ticket-title-disclosure]');
      const slug = disclosure?.dataset.ticketSlug;
      const presentation = disclosure?.dataset.presentation;
      if (!disclosure || !slug || (presentation !== 'sidebar' && presentation !== 'reader')) return;
      const expanded = toggleTicketTitleExpanded(slug, presentation);
      disclosure.dataset.titleExpanded = String(expanded);
      target.setAttribute('aria-expanded', String(expanded));
      target.textContent = expanded ? 'Collapse title' : 'Show full title';
    }),
  );
  lifetime.add(
    wireTicketTitleEditor(document.body, {
      begin: (target) => {
        flushDraftInputs();
        beginTitleEdit(target);
      },
      input: (value, field) => {
        // The wrapping title grows from its frame's mirror text; keep that immediate while the
        // draft signal waits for the typing pause (HS2-EQTXTJ, HS2-98ZVPE).
        field.closest<HTMLElement>('[data-title-mirror]')?.setAttribute('data-title-mirror', value);
        titleDraftInput.schedule(value);
        if (value.trim()) titleAutosave.schedule(value);
      },
      finish: () => {
        flushDraftInputs();
        if (!titleDraft.value.trim()) return;
        void titleAutosave.flush().then((saved) => {
          if (saved) titleEditingSurface.value = undefined;
        });
      },
    }),
  );
  // Escape in an inline editor finishes that edit (its focusout autosaves) and stops there, so the
  // narrow inspector overlay, which Kerf's Workbench dismisses from a document keydown, stays open
  // until a second Escape. The batch label restores its own value on Escape (HS2-Q2T01A).
  const finishInlineEditOnEscape = (event: Event) => {
    const field = event.target instanceof Element ? event.target : null,
      outcome = inlineEditorEscape(
        event as KeyboardEvent,
        field,
        ATTACHMENTS_AND_GALLERY_TARGETS.attachmentBatchLabelField.selector,
      );
    if (outcome === 'none') return;
    event.stopPropagation();
    if (outcome === 'finish') (field as HTMLElement).blur();
  };
  lifetime.add(
    delegate(
      document.body,
      'keydown',
      INSPECTOR_AND_EDITOR_TARGETS.ticketInspectorHeader.selector,
      finishInlineEditOnEscape,
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'keydown',
      INSPECTOR_AND_EDITOR_TARGETS.ticketInspectorBody.selector,
      finishInlineEditOnEscape,
    ),
  );
  function setSelectedTags(tags: string[]) {
    const ticket = editTicket();
    if (!ticket || !canUpdateEditTicket()) return;
    setEditTicket({ ...ticket, tags });
    tagsAutosave.schedule(tags);
  }
  // Kerf's AnchoredSurface owns the Add tag surface's chrome, position, focus, and dismissal; one
  // surface is open at a time and Escape or an outside click closes it (HS2-Q4JE3V). The field's
  // Enter and focusout reach wireTicketTagEditor, which delegates from the body that hosts it.
  let tagSurface: OverlayHandle | undefined;
  lifetime.add(() => tagSurface?.close());
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.openTicketTagPopover.selector, (_event, target) => {
      tagSurface?.close();
      const surface = openTicketTagSurface(target);
      tagSurface = surface;
      void surface.result.then(() => {
        if (tagSurface === surface) tagSurface = undefined;
      });
    }),
  );
  lifetime.add(
    wireTicketTagEditor(document.body, {
      add: (value) => {
        setSelectedTags(addTicketTag(editTicket()?.tags ?? [], value));
      },
      flush: () => {
        void tagsAutosave.flush();
      },
      remove: (tag) => {
        setSelectedTags(removeTicketTag(editTicket()?.tags ?? [], tag));
      },
    }),
  );
  lifetime.add(
    delegateCapture(document.body, 'pointerdown', '*', (event, target) => {
      const active = document.activeElement;
      if (
        event.defaultPrevented ||
        !(active instanceof HTMLTextAreaElement) ||
        active.name !== 'markdown-source' ||
        active.closest('[data-component="markdown-editor"]')?.contains(target)
      )
        return;
      dependencies.pointerDetailsReader = isReaderSurface(active);
      dependencies.pointerDetailsFinish = undefined;
    }),
  );
  // A single click enters editing; links and controls inside the rendered Markdown keep their own
  // action instead (HS2-H1K9YY). The repeat press of a habitual double-click keeps focus in the
  // editor its first click opened.
  lifetime.add(
    wireMarkdownDetailsEditor(document.body, {
      begin: (target) => {
        flushDraftInputs();
        beginDetailsEdit(isReaderSurface(target), linkedReaderFrame(target));
      },
      input: (field) => {
        const frame = linkedReaderFrame(field),
          value = field.value;
        if (frame) {
          linkedDetailsDraftInput.schedule({ frameId: frame.id, value });
          linkedReaderSaves(frame.id).details.schedule(value);
          return;
        }
        const reader = isReaderSurface(field),
          autosave = reader ? readerDetailsAutosave : detailsAutosave;
        detailsDraftInput.schedule({ reader, value });
        autosave.schedule(value);
      },
      focusLeave: (field, next, insideEditor) => {
        flushDraftInputs();
        const frame = linkedReaderFrame(field);
        if (frame) {
          void linkedReaderSaves(frame.id)
            .details.flush()
            .then((saved) => {
              if (saved)
                replaceLinkedReaderFrame(frame.id, (current) => ({
                  ...current,
                  edit: { ...current.edit, detailsMode: 'preview' },
                }));
            });
          return;
        }
        if (insideEditor || (next instanceof Element && next.closest('[data-component="ticket-field-conflict"]'))) {
          dependencies.pointerDetailsReader = undefined;
          return;
        }
        const reader = isReaderSurface(field);
        if (dependencies.pointerDetailsReader === reader) {
          dependencies.pointerDetailsFinish = beginDetailsFinish(reader);
          return;
        }
        setTimeout(() => void finishDetailsEdit(reader), 0);
      },
    }),
  );
  function beginNoteEdit(id: string, reader = false, frame?: TicketReaderFrame, feedbackPrompt = false) {
    const note = (frame?.ticket ?? selectedTicket.value)?.notes.find((item) => item.id === id);
    if (!note) return;
    const scope = responseScope(frame);
    if (scope) feedbackResponseDrafts.clear(feedbackResponseDrafts.get(scope));
    // A FEEDBACK NEEDED prompt is replied to with a new note from the reader's response editor
    // (HS2-MB2Q10); only the explicit Edit Note action opens the prompt itself (HS2-ZXGEFE).
    if (note.kind === 'feedback_needed' && !feedbackPrompt) return;
    if (note.kind === 'feedback_needed') beginFeedbackPromptEdit(id);
    if (frame) {
      if (!frame.capabilities.note_edit) return;
      const restored = restoreTicketDraft('note', note.text, id);
      replaceLinkedReaderFrame(frame.id, (current) => ({
        ...current,
        edit: {
          ...current.edit,
          editingNoteId: id,
          responseRequestId: undefined,
          noteDraft: restored?.draft ?? note.text,
          noteBase: restored?.base ?? note.text,
          noteGeneration: current.edit.noteGeneration + 1,
        },
      }));
      if (restored) linkedReaderSaves(frame.id).note.schedule({ id, value: restored.draft });
      queueMicrotask(() =>
        document
          .querySelector<HTMLElement>(`[data-reader-frame-id="${frame.id}"] [name="note-body"][data-note-id="${id}"]`)
          ?.focus(),
      );
      return;
    }
    const editing = reader ? readerEditingNoteId : editingNoteId,
      draft = reader ? readerNoteDraft : noteDraft;
    editing.value = id;
    const initial = note.text,
      restoredNote = restoreTicketDraft('note', note.text, id);
    draft.value = restoredNote?.draft ?? initial;
    if (reader) dependencies.readerNoteDraftBase = restoredNote?.base ?? initial;
    else dependencies.noteDraftBase = restoredNote?.base ?? initial;
    if (restoredNote) (reader ? readerNoteAutosave : noteAutosave).schedule({ id, value: draft.value });
    queueMicrotask(() =>
      activeTicketSurface().querySelector<HTMLElement>(`[name="note-body"][data-note-id="${id}"]`)?.focus(),
    );
  }
  const newNoteTimers = new Map<string, ReturnType<typeof setTimeout>>(),
    creatingNotes = new Set<string>();
  const composerKey = (target: HTMLElement) => {
    const frame = linkedReaderFrame(target);
    return frame
      ? `frame:${frame.id}`
      : selectedTicket.value
        ? `workspace:${selectedTicket.value.qualified_id}`
        : undefined;
  };
  function cancelNewNoteTimer(key: string | undefined) {
    if (!key) return;
    const timer = newNoteTimers.get(key);
    if (timer) clearTimeout(timer);
    newNoteTimers.delete(key);
  }
  function saveNewNote(key: string | undefined) {
    if (key && creatingNotes.has(key)) {
      cancelNewNoteTimer(key);
      newNoteTimers.set(
        key,
        setTimeout(() => {
          saveNewNote(key);
        }, 1_000),
      );
      return;
    }
    cancelNewNoteTimer(key);
    if (!key) return;
    flushDraftInputs();
    if (key.startsWith('frame:')) {
      const frameId = key.slice('frame:'.length),
        frame = linkedReaderStack.value.find((item) => item.id === frameId),
        text = frame?.edit.newNoteDraft.trim();
      if (!frame?.edit.composingNote || !frame.capabilities.notes || !text) return;
      creatingNotes.add(key);
      const save = linkedReaderSaves(frameId).respond(text);
      setFrameComposer(frameId, false, '');
      void save.then((saved) => {
        creatingNotes.delete(key);
        if (!saved && linkedReaderStack.value.some((item) => item.id === frameId && !item.edit.composingNote))
          setFrameComposer(frameId, true, text);
      });
      return;
    }
    if (key !== `workspace:${selectedTicket.value?.qualified_id}` || !composingNote.value || !canAddNotes()) return;
    const text = newNoteDraft.value.trim();
    if (!text) return;
    creatingNotes.add(key);
    composingNote.value = false;
    newNoteDraft.value = '';
    scheduleProjectSessionPersistence();
    void updateSelected({ note: text }).then((saved) => {
      creatingNotes.delete(key);
      if (!saved && key === `workspace:${selectedTicket.value?.qualified_id}` && !composingNote.value) {
        composingNote.value = true;
        newNoteDraft.value = text;
        scheduleProjectSessionPersistence();
      }
    });
  }
  lifetime.add(() => {
    for (const timer of newNoteTimers.values()) clearTimeout(timer);
    newNoteTimers.clear();
  });
  lifetime.add(
    wireNoteComposer(document.body, {
      open: (target) => {
        cancelNewNoteTimer(composerKey(target));
        const frame = linkedReaderFrame(target);
        if (frame) {
          // A linked reader frame owns its composer, so it never borrows the workspace draft (HS2-ANRXAE).
          if (!frame.capabilities.notes) return;
          setFrameComposer(frame.id, true, '');
          queueMicrotask(() =>
            document
              .querySelector<HTMLElement>(`[data-reader-frame-id="${CSS.escape(frame.id)}"] [name="new-note-body"]`)
              ?.focus(),
          );
          return;
        }
        if (!canAddNotes()) return;
        editingNoteId.value = undefined;
        composingNote.value = true;
        newNoteDraft.value = '';
        scheduleProjectSessionPersistence();
        queueMicrotask(() => activeTicketSurface().querySelector<HTMLElement>('[name="new-note-body"]')?.focus());
      },
      setDraft: (value, target) => {
        const frame = linkedReaderFrame(target);
        if (frame) linkedNewNoteDraftInput.schedule({ frameId: frame.id, value });
        else newNoteDraftInput.schedule(value);
        const key = composerKey(target);
        cancelNewNoteTimer(key);
        if (key && value.trim())
          newNoteTimers.set(
            key,
            setTimeout(() => {
              saveNewNote(key);
            }, 10_000),
          );
      },
      cancel: (target) => {
        cancelNewNoteTimer(composerKey(target));
        flushDraftInputs();
        const frame = linkedReaderFrame(target);
        if (frame) {
          setFrameComposer(frame.id, false, '');
          return;
        }
        composingNote.value = false;
        newNoteDraft.value = '';
        scheduleProjectSessionPersistence();
      },
      submit: (target) => {
        saveNewNote(composerKey(target));
      },
      finish: (target) => {
        saveNewNote(composerKey(target));
      },
    }),
  );
  // Inline feedback replies and choice selections belong to the reader that shows the prompt: the
  // workspace reader keeps them in its session-persisted signals, and each linked frame in its own
  // edit state, so two readers never share a draft or a details-prompt key (HS2-ANRXAE).
  const choiceAnchorKey = (noteId: string, frameId?: string) => (frameId ? `${frameId}\u0000${noteId}` : noteId);
  function feedbackReplies(noteId: string, frameId?: string): InlineFeedbackReply[] {
    const frame = frameId ? linkedReaderStack.value.find((item) => item.id === frameId) : undefined;
    const replies: Partial<Record<string, InlineFeedbackReply[]>> = frame
      ? frame.edit.inlineFeedbackReplies
      : readerInlineFeedbackReplies.value;
    return replies[noteId] ?? [];
  }
  function setFeedbackReplies(noteId: string, replies: InlineFeedbackReply[], frameId?: string) {
    if (frameId) {
      replaceLinkedReaderFrame(frameId, (current) => ({
        ...current,
        edit: {
          ...current.edit,
          inlineFeedbackReplies: { ...current.edit.inlineFeedbackReplies, [noteId]: replies },
        },
      }));
      return;
    }
    readerInlineFeedbackReplies.value = { ...readerInlineFeedbackReplies.value, [noteId]: replies };
    scheduleProjectSessionPersistence();
  }
  function feedbackChoiceSelection(noteId: string, frameId?: string): string[] {
    const frame = frameId ? linkedReaderStack.value.find((item) => item.id === frameId) : undefined;
    const selections: Partial<Record<string, string[]>> = frame
      ? frame.edit.feedbackChoiceSelections
      : readerFeedbackChoiceSelections.value;
    return selections[noteId] ?? [];
  }
  function focusInlineFeedbackReply(noteId: string, offset: number, frameId?: string) {
    captureResponse(noteId, frameId ? linkedReaderStack.value.find((frame) => frame.id === frameId) : undefined, true);
    const existing = feedbackReplies(noteId, frameId);
    if (!existing.some((reply) => reply.offset === offset))
      setFeedbackReplies(noteId, [...existing, { offset, text: '' }], frameId);
    else if (!frameId) scheduleProjectSessionPersistence();
    const field = `[name="inline-feedback-response"][data-note-id="${noteId}"][data-offset="${offset}"]`;
    queueMicrotask(() =>
      (frameId
        ? document.querySelector<HTMLElement>(`[data-reader-frame-id="${CSS.escape(frameId)}"] ${field}`)
        : activeTicketSurface().querySelector<HTMLElement>(field)
      )?.focus(),
    );
  }
  function responseScope(frame?: TicketReaderFrame) {
    const current = project(),
      ticket = frame?.ticket ?? selectedTicket.value;
    return ticket && (frame || current)
      ? {
          projectId: frame?.projectId ?? current!.id,
          qualifiedId: ticket.qualified_id,
          surface: frame?.id ?? 'workspace-reader',
        }
      : undefined;
  }
  function captureResponse(id: string, frame?: TicketReaderFrame, edited = false, draft?: string) {
    const scope = responseScope(frame),
      note = (frame?.ticket ?? selectedTicket.value)?.notes.find((item) => item.id === id);
    if (!scope) return undefined;
    const existing = feedbackResponseDrafts.get(scope);
    // Description feedback is an existing regular append, not a note response_to authority (HS2-VBFSZJ).
    if (id === DETAILS_FEEDBACK_ID) {
      feedbackResponseDrafts.clear(existing);
      return undefined;
    }
    if (existing?.request.id === id) return edited ? feedbackResponseDrafts.touch(existing, draft) : existing;
    if (!note || note.kind !== 'feedback_needed') return undefined;
    return feedbackResponseDrafts.begin(scope, note, draft);
  }
  function clearFeedbackReplyState(id: string, frameId?: string) {
    readerFeedbackChoiceAnchors.delete(choiceAnchorKey(id, frameId));
    if (frameId) {
      replaceLinkedReaderFrame(frameId, (current) => ({ ...current, edit: clearFrameFeedbackReply(current.edit, id) }));
      return;
    }
    readerInlineFeedbackReplies.value = { ...readerInlineFeedbackReplies.value, [id]: [] };
    readerFeedbackChoiceSelections.value = { ...readerFeedbackChoiceSelections.value, [id]: [] };
  }
  /** Save or answer a note inside a linked reader frame, against that frame's own ticket. */
  function saveLinkedReaderNote(frame: TicketReaderFrame, target: HTMLElement) {
    inlineFeedbackReplyInput.flush();
    const id =
      data(target).noteResponse === 'true' ? data(target).noteId : (frame.edit.editingNoteId ?? data(target).noteId);
    if (!id) return;
    const owner = captureResponse(id, frame),
      note = frame.ticket.notes.find((item) => item.id === id) ?? owner?.request;
    if (!note) return;
    const response = data(target).noteResponse === 'true' || Boolean(owner) || note.kind === 'feedback_needed',
      value = response
        ? combineFeedbackReply(
            note.text,
            feedbackReplies(id, frame.id),
            owner ? (owner.draft ?? '') : frame.edit.noteDraft,
            feedbackChoiceSelection(id, frame.id),
          )
        : frame.edit.noteDraft;
    if (!value.trim()) return;
    const saves = linkedReaderSaves(frame.id),
      saved = response ? saves.respond(value, id) : saves.note.flush();
    void saved.then((ok) => {
      if (!ok || (owner && feedbackResponseDrafts.get(owner) !== owner)) return;
      if (owner) feedbackResponseDrafts.clear(owner);
      replaceLinkedReaderFrame(frame.id, (current) => ({
        ...current,
        edit: { ...current.edit, editingNoteId: undefined, noteDraft: '', noteBase: '' },
      }));
      if (response) clearFeedbackReplyState(id, frame.id);
    });
  }
  function feedbackSource(noteId: string, frameId?: string) {
    const frame = frameId ? linkedReaderStack.value.find((item) => item.id === frameId) : undefined,
      ticket = frame?.ticket ?? selectedTicket.value,
      scope = responseScope(frame),
      owner = scope ? feedbackResponseDrafts.get(scope) : undefined;
    return noteId === DETAILS_FEEDBACK_ID
      ? ticket?.details
      : owner?.request.id === noteId
        ? owner.request.text
        : ticket?.notes.find((item) => item.id === noteId)?.text;
  }
  function inlineFeedbackClickOffset(event: MouseEvent, target: Element) {
    const start = Number(data(target).segmentStart),
      end = Number(data(target).segmentEnd),
      noteId = data(target).noteId!,
      source = feedbackSource(noteId, linkedReaderFrame(target)?.id)?.slice(start, end) ?? '',
      caretDocument = document as Document & {
        caretPositionFromPoint?: (x: number, y: number) => { offsetNode: Node; offset: number } | null;
      },
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      position = caretDocument.caretPositionFromPoint?.(event.clientX, event.clientY);
    if (!position || !target.contains(position.offsetNode)) return end;
    const range = document.createRange();
    range.selectNodeContents(target);
    range.setEnd(position.offsetNode, position.offset);
    // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
    return start + sourceOffsetForVisibleOffset(source, target.textContent ?? '', range.toString().length);
  }
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.addInlineFeedbackReply.selector, (event, target) => {
      if ((event.target as Element).closest('a')) return;
      focusInlineFeedbackReply(
        data(target).noteId!,
        inlineFeedbackClickOffset(event as MouseEvent, target),
        linkedReaderFrame(target)?.id,
      );
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'keydown',
      INSPECTOR_AND_EDITOR_ACTIONS.addInlineFeedbackReply.selector,
      (event, target) => {
        if ((event.target as Element).closest('a') || !['Enter', ' '].includes((event as KeyboardEvent).key)) return;
        event.preventDefault();
        focusInlineFeedbackReply(data(target).noteId!, Number(data(target).segmentEnd), linkedReaderFrame(target)?.id);
      },
    ),
  );
  // Keystrokes coalesce for 150 ms before rebuilding the reply map (HS2-2YW8R3); blur, removal, and
  // every submit path flush first so no typed text is lost or answered stale.
  const inlineFeedbackReplyInput = createKeyedCoalescedInput<{
    noteId: string;
    offset: number;
    text: string;
    frameId?: string;
  }>(
    ({ noteId, offset, frameId }) => `${frameId ?? ''}\u0000${noteId}:${offset}`,
    ({ noteId, offset, text, frameId }) => {
      setFeedbackReplies(
        noteId,
        feedbackReplies(noteId, frameId).map((reply) => (reply.offset === offset ? { ...reply, text } : reply)),
        frameId,
      );
    },
  );
  lifetime.add(() => {
    inlineFeedbackReplyInput.flush();
  });
  lifetime.add(
    delegate(
      document.body,
      'input',
      INSPECTOR_AND_EDITOR_TARGETS.inlineFeedbackResponseField.selector,
      (_event, target) => {
        captureResponse(data(target).noteId!, linkedReaderFrame(target), true);
        inlineFeedbackReplyInput.schedule({
          noteId: data(target).noteId!,
          offset: Number(data(target).offset),
          text: (target as HTMLTextAreaElement).value,
          frameId: linkedReaderFrame(target)?.id,
        });
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'focusout', INSPECTOR_AND_EDITOR_TARGETS.inlineFeedbackResponseField.selector, () => {
      inlineFeedbackReplyInput.flush();
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      INSPECTOR_AND_EDITOR_ACTIONS.removeInlineFeedbackReply.selector,
      (_event, target) => {
        inlineFeedbackReplyInput.flush();
        const noteId = data(target).noteId!,
          offset = Number(data(target).offset),
          frameId = linkedReaderFrame(target)?.id;
        setFeedbackReplies(
          noteId,
          feedbackReplies(noteId, frameId).filter((reply) => reply.offset !== offset),
          frameId,
        );
      },
    ),
  );
  function toggleFeedbackChoice(
    noteId: string,
    choiceId: string,
    modifiers: FeedbackChoiceModifiers,
    target: HTMLElement,
  ) {
    const frame = linkedReaderFrame(target),
      frameId = frame?.id;
    captureResponse(noteId, frame, true);
    const source = feedbackSource(noteId, frameId),
      choices = source && parseFeedbackChoices(source);
    if (!choices) return;
    const anchorKey = choiceAnchorKey(noteId, frameId),
      next = updateFeedbackChoiceDocumentSelection(
        choices,
        feedbackChoiceSelection(noteId, frameId),
        choiceId,
        readerFeedbackChoiceAnchors.get(anchorKey),
        modifiers,
      );
    if (next.anchor) readerFeedbackChoiceAnchors.set(anchorKey, next.anchor);
    if (frameId) {
      replaceLinkedReaderFrame(frameId, (current) => ({
        ...current,
        edit: {
          ...current.edit,
          feedbackChoiceSelections: { ...current.edit.feedbackChoiceSelections, [noteId]: next.selected },
        },
      }));
      return;
    }
    readerFeedbackChoiceSelections.value = { ...readerFeedbackChoiceSelections.value, [noteId]: next.selected };
    scheduleProjectSessionPersistence();
  }
  lifetime.add(wireFeedbackChoices(document.body, { toggle: toggleFeedbackChoice }));
  function deleteWorkspaceNote(noteId: string | undefined) {
    const current = project(),
      ticket = selectedTicket.value;
    if (!current || !ticket || !noteId || !canDeleteNotes()) return;
    void api()
      .deleteCheckoutNote(current.id, ticket.qualified_id, noteId)
      .then((result) => {
        selectedTicket.value = result.ticket;
        editingNoteId.value = undefined;
        noteDraft.value = '';
        return refreshProject();
      })
      .catch((reason: unknown) => {
        error.value = reason instanceof Error ? reason.message : String(reason);
      });
  }
  lifetime.add(
    wireNoteEditor(document.body, {
      begin: (noteId, target) => {
        flushDraftInputs();
        beginNoteEdit(noteId, isReaderSurface(target), linkedReaderFrame(target));
      },
      beginPrompt: (noteId, target) => {
        flushDraftInputs();
        beginNoteEdit(noteId, isReaderSurface(target), linkedReaderFrame(target), true);
      },
      input: (field, id, response) => {
        if (!field.isConnected) return;
        const frame = linkedReaderFrame(field),
          value = field.value;
        if (frame && id) {
          const responseOwner = response ? captureResponse(id, frame, true, value) : undefined;
          if (response && !responseOwner && id !== DETAILS_FEEDBACK_ID) return;
          linkedNoteDraftInput.schedule({ frameId: frame.id, id, value, responseOwner });
          // A feedback response is only a draft until Respond creates its own note (HS2-MB2Q10).
          if (!response) linkedReaderSaves(frame.id).note.schedule({ id, value });
          return;
        }
        const responseOwner =
          response && id && isReaderSurface(field) ? captureResponse(id, undefined, true, value) : undefined;
        if (response && !responseOwner && id !== DETAILS_FEEDBACK_ID) return;
        const reader = isReaderSurface(field),
          editing = reader ? readerEditingNoteId : editingNoteId,
          autosave = reader ? readerNoteAutosave : noteAutosave;
        if (editing.value !== id) editing.value = id;
        noteDraftInput.schedule({ reader, value, responseOwner });
        const prompt =
          selectedTicket.value?.notes.find((item) => item.id === editing.value)?.kind === 'feedback_needed';
        // A prompt autosaves only while it is explicitly open for editing (HS2-ZXGEFE).
        if (!response && (!prompt || isFeedbackPromptEdit(id)) && editing.value)
          autosave.schedule({ id: editing.value, value });
        else scheduleProjectSessionPersistence();
      },
      finish: (field) => {
        flushDraftInputs();
        const frame = linkedReaderFrame(field);
        if (frame) {
          const id = frame.edit.editingNoteId ?? field.dataset.noteId,
            note = frame.ticket.notes.find((item) => item.id === id);
          if (note?.kind === 'regular' && frame.capabilities.note_delete && !field.value.trim()) {
            linkedReaderSaves(frame.id).note.cancel();
            deleteLinkedReaderNote(frame, id);
            return;
          }
          void linkedReaderSaves(frame.id)
            .note.flush()
            .then((saved) => {
              if (saved)
                replaceLinkedReaderFrame(frame.id, (current) => ({
                  ...current,
                  edit: { ...current.edit, editingNoteId: undefined, noteDraft: '', noteBase: '' },
                }));
            });
          return;
        }
        const reader = isReaderSurface(field),
          editing = reader ? readerEditingNoteId : editingNoteId,
          draft = reader ? readerNoteDraft : noteDraft,
          autosave = reader ? readerNoteAutosave : noteAutosave;
        const finishedId = editing.value;
        const note = selectedTicket.value?.notes.find((item) => item.id === finishedId);
        if (note?.kind === 'regular' && canDeleteNotes() && !field.value.trim()) {
          autosave.cancel();
          deleteWorkspaceNote(finishedId);
          return;
        }
        void autosave.flush().then((saved) => {
          if (saved) {
            endFeedbackPromptEdit(finishedId);
            editing.value = undefined;
            draft.value = '';
          }
        });
      },
      save: (target) => {
        flushDraftInputs();
        inlineFeedbackReplyInput.flush();
        const frame = linkedReaderFrame(target);
        if (frame) {
          saveLinkedReaderNote(frame, target);
          return;
        }
        const reader = isReaderSurface(target),
          editing = reader ? readerEditingNoteId : editingNoteId,
          draft = reader ? readerNoteDraft : noteDraft,
          id = data(target).noteResponse === 'true' ? data(target).noteId : (editing.value ?? data(target).noteId),
          note = selectedTicket.value?.notes.find((item) => item.id === id),
          response =
            data(target).noteResponse === 'true' ||
            note?.kind === 'feedback_draft' ||
            (note?.kind === 'feedback_needed' && !isFeedbackPromptEdit(id)),
          owner = response && reader && id ? captureResponse(id) : undefined,
          source = (id ? feedbackSource(id) : undefined) ?? owner?.request.text,
          value =
            source && response
              ? combineFeedbackReply(
                  source,
                  readerInlineFeedbackReplies.value[id ?? ''] ?? [],
                  owner ? (owner.draft ?? '') : draft.value,
                  readerFeedbackChoiceSelections.value[id ?? ''] ?? [],
                )
              : draft.value;
        if (!id || !value.trim()) return;
        const patch = response
          ? {
              note: value,
              note_kind: 'regular',
              ...(owner || note?.kind === 'feedback_needed' ? { response_to: owner?.request.id ?? id } : {}),
            }
          : noteEditPatch(linkedReaderFrame(target)?.ticket ?? selectedTicket.value, id, value);
        void updateSelected(patch).then((saved) => {
          if (saved && (!owner || feedbackResponseDrafts.get(owner) === owner)) {
            if (owner) feedbackResponseDrafts.clear(owner);
            endFeedbackPromptEdit(id);
            editing.value = undefined;
            draft.value = '';
            clearFeedbackReplyState(id);
          }
        });
      },
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.dismissFeedback.selector, (_event, target) => {
      const id = data(target).noteId;
      if (!id) return;
      inlineFeedbackReplyInput.cancel();
      const frame = linkedReaderFrame(target),
        scope = responseScope(frame),
        owner = scope ? feedbackResponseDrafts.get(scope) : undefined,
        editingAtDismissal = frame ? frame.edit.editingNoteId : readerEditingNoteId.value,
        // A linked reader frame answers on its own ticket, not the workspace selection (HS2-04XTV5).
        saved = dismissFeedbackRequest(id, frame?.id);
      void saved.then((ok) => {
        if (!ok || !scope || feedbackResponseDrafts.get(scope) !== owner) return;
        const currentFrame = frame ? linkedReaderStack.value.find((item) => item.id === frame.id) : undefined;
        if (frame && !currentFrame?.open) return;
        if (
          !frame &&
          (!readerOpen.value ||
            project()?.id !== scope.projectId ||
            selectedTicket.value?.qualified_id !== scope.qualifiedId)
        )
          return;
        if (!owner && (frame ? currentFrame?.edit.editingNoteId : readerEditingNoteId.value) !== editingAtDismissal)
          return;
        feedbackResponseDrafts.clear(owner);
        if (!frame && readerEditingNoteId.value === id) {
          readerEditingNoteId.value = undefined;
          readerNoteDraft.value = '';
        }
        clearFeedbackReplyState(id, frame?.id);
      });
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.deleteNote.selector, (_event, target) => {
      const frame = linkedReaderFrame(target);
      if (frame) {
        deleteLinkedReaderNote(frame, data(target).noteId);
        return;
      }
      deleteWorkspaceNote(data(target).noteId);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.loadNextTicketPage.selector, (_event, target) => {
      const columnId = target.closest<HTMLElement>('[data-component="ticket-board-column"]')?.dataset.columnId;
      if (
        columnId &&
        viewMode.value === 'board' &&
        isPerColumnBoardView(selectedView.value, workspaceSearchActive()) &&
        boardColumnStatus(columnId)
      )
        void loadBoardColumnMore(columnId);
      else void loadNextTicketPage();
    }),
  );
  /** Delete a note from a linked reader frame's own ticket through its owning project (HS2-ANRXAE). */
  function deleteLinkedReaderNote(frame: TicketReaderFrame, noteId: string | undefined) {
    if (!noteId || !frame.capabilities.note_delete) return;
    void new Api(frame.apiPath)
      .deleteCheckoutNote(frame.projectId, frame.ticket.qualified_id, noteId)
      .then((result) => {
        replaceLinkedReaderFrame(frame.id, (current) =>
          reconcileTicketReaderFrame(
            current.edit.editingNoteId === noteId
              ? { ...current, edit: { ...current.edit, editingNoteId: undefined, noteDraft: '', noteBase: '' } }
              : current,
            result.ticket,
          ),
        );
        clearFeedbackReplyState(noteId, frame.id);
      })
      .catch((reason: unknown) => {
        error.value = reason instanceof Error ? reason.message : String(reason);
      });
  }
  function beginBlockedReasonEdit(reader = false, frame?: TicketReaderFrame) {
    if (frame) {
      if (!frame.capabilities.update) return;
      const restored = restoreTicketDraft('blocked_reason', frame.ticket.blocked_reason ?? '');
      replaceLinkedReaderFrame(frame.id, (current) => ({
        ...current,
        edit: {
          ...current.edit,
          blockedReasonEditing: true,
          blockedReasonDraft: restored?.draft ?? current.ticket.blocked_reason ?? '',
          blockedReasonBase: restored?.base ?? current.ticket.blocked_reason ?? '',
          blockedReasonGeneration: current.edit.blockedReasonGeneration + 1,
        },
      }));
      if (restored) linkedReaderSaves(frame.id).blocked.schedule(restored.draft);
      queueMicrotask(() =>
        document.querySelector<HTMLElement>(`[data-reader-frame-id="${frame.id}"] [name="blocked-reason"]`)?.focus(),
      );
      return;
    }
    const draft = reader ? readerBlockedReasonDraft : blockedReasonDraft,
      editing = reader ? readerBlockedReasonEditing : blockedReasonEditing;
    const currentReason = selectedTicket.value?.blocked_reason ?? '',
      restoredReason = restoreTicketDraft('blocked_reason', currentReason);
    draft.value = restoredReason?.draft ?? currentReason;
    if (reader) dependencies.readerBlockedReasonDraftBase = restoredReason?.base ?? currentReason;
    else dependencies.blockedReasonDraftBase = restoredReason?.base ?? currentReason;
    editing.value = true;
    if (restoredReason) (reader ? readerBlockedReasonAutosave : blockedReasonAutosave).schedule(draft.value);
    queueMicrotask(() => activeTicketSurface().querySelector<HTMLElement>('[name="blocked-reason"]')?.focus());
  }
  lifetime.add(
    wireBlockedReasonEditor(document.body, {
      begin: (target, trigger) => {
        flushDraftInputs();
        // The keyboard path has always opened the inline editor even inside a linked reader frame.
        beginBlockedReasonEdit(isReaderSurface(target), trigger === 'key' ? undefined : linkedReaderFrame(target));
      },
      input: (value, target) => {
        const frame = linkedReaderFrame(target);
        if (frame) {
          linkedBlockedReasonDraftInput.schedule({ frameId: frame.id, value });
          linkedReaderSaves(frame.id).blocked.schedule(value);
          return;
        }
        const reader = isReaderSurface(target),
          autosave = reader ? readerBlockedReasonAutosave : blockedReasonAutosave;
        blockedReasonDraftInput.schedule({ reader, value });
        autosave.schedule(value);
      },
      finish: (target) => {
        flushDraftInputs();
        const frame = linkedReaderFrame(target);
        if (frame) {
          void linkedReaderSaves(frame.id)
            .blocked.flush()
            .then((saved) => {
              if (saved)
                replaceLinkedReaderFrame(frame.id, (current) => ({
                  ...current,
                  edit: { ...current.edit, blockedReasonEditing: false },
                }));
            });
          return;
        }
        const reader = isReaderSurface(target),
          editing = reader ? readerBlockedReasonEditing : blockedReasonEditing,
          autosave = reader ? readerBlockedReasonAutosave : blockedReasonAutosave;
        void autosave.flush().then((saved) => {
          if (saved) editing.value = false;
        });
      },
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'pointerup',
      'textarea[name="markdown-source"], textarea[name="blocked-reason"], textarea[name="note-body"], textarea[name="new-ticket-details"]',
      (_event, target) => {
        const textarea = target as HTMLTextAreaElement,
          kind = ticketEditorKind(textarea.name),
          height = manuallyResizedTicketEditorHeight(textarea.style.height);
        if (kind && height !== undefined)
          saveTicketEditorSize(
            localStorage,
            document.documentElement.style,
            kind,
            isReaderSurface(target) ? 'reader' : 'sidebar',
            height,
          );
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.openTicketReader.selector, (_event, target) => {
      presentTicketReaderDialog('workspace-reader', target as HTMLElement, () => {
        readerOpen.value = true;
        if (readerTab.value === 'code-review' && !codeReviewLoading.value) void refreshCodeReview();
      });
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.respondToFeedback.selector, (_event, target) => {
      const noteId = data(target).noteId;
      if (!noteId) return;
      readerTab.value = 'info';
      scheduleProjectSessionPersistence();
      presentTicketReaderDialog(
        'workspace-reader',
        target as HTMLElement,
        () => {
          readerOpen.value = true;
        },
        () => {
          const reader = readerDialog('workspace-reader'),
            surface =
              noteId === DETAILS_FEEDBACK_ID
                ? reader?.querySelector<HTMLElement>('[data-details-feedback="true"]')
                : [...(reader?.querySelectorAll<HTMLElement>('[data-component="note-card"]') ?? [])].find(
                    (candidate) => candidate.dataset.noteId === noteId,
                  );
          surface?.querySelector<HTMLElement>('[name="note-body"]')?.focus({ preventScroll: true });
          surface?.scrollIntoView({ block: 'center' });
        },
      );
    }),
  );
  lifetime.add(
    delegateCapture(
      document.body,
      'kui-request-close',
      INSPECTOR_AND_EDITOR_TARGETS.ticketReaderDialog.selector,
      (event, target) => {
        if (event.target !== target) return;
        approveTicketReaderClose(target as TicketReaderDialogElement);
      },
    ),
  );
  lifetime.add(
    delegateCapture(
      document.body,
      'close',
      INSPECTOR_AND_EDITOR_TARGETS.ticketReaderDialog.selector,
      (event, target) => {
        if (event.target === target) finishTicketReaderClose(target as TicketReaderDialogElement);
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.toggleReaderTextSize.selector, () => {
      readerLargeText.value = !readerLargeText.value;
      localStorage.setItem('hotsheet.reader.large-text', String(readerLargeText.value));
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.setInspectorTab.selector, (_event, target) => {
      const tab = data(target).tabId as InspectorTab,
        frameId = target.closest<HTMLElement>('[data-reader-frame-id]')?.dataset.readerFrameId;
      if (frameId && frameId !== 'workspace-reader')
        linkedReaderStack.value = linkedReaderStack.value.map((frame) =>
          frame.id === frameId ? { ...frame, activeTab: tab } : frame,
        );
      else if (isReaderSurface(target)) readerTab.value = tab;
      else inspectorTab.value = tab;
      scheduleProjectSessionPersistence();
      if (tab === 'timeline' && !frameId && !isReaderSurface(target)) void refreshTicketUsage();
      if (tab !== 'code-review') return;
      if (frameId && frameId !== 'workspace-reader') void refreshLinkedCodeReview(frameId);
      else if (!codeReviewLoading.value) void refreshCodeReview();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.openCodeReview.selector, (_event, target) => {
      const frame = linkedReaderFrame(target);
      if (frame) {
        openLinkedCodeReview(frame, target);
        return;
      }
      const current = project(),
        ticket = selectedTicket.value,
        reviewTarget = codeReviewTarget(data(target));
      if (!current || !ticket || !reviewTarget) return;
      codeReviewMessage.value = 'Opening diff tool…';
      void new Api(current.apiPath)
        .openCodeReview(current.id, ticket.qualified_id, reviewTarget)
        .then(() => {
          if (project()?.id === current.id && selectedTicket.value?.id === ticket.id) {
            codeReviewMessage.value = '';
            showToast(`Opened in ${codeReview.value?.difftool ?? 'the configured diff tool'}.`);
          }
        })
        .catch((reason: unknown) => {
          if (reason instanceof HostOnlyActionError) {
            // The inline message can sit below the fold of a long commit list (HS2-F78B1S).
            codeReviewMessage.value = '';
            showToast(reason.message);
          } else if (project()?.id === current.id && selectedTicket.value?.id === ticket.id)
            codeReviewMessage.value = reason instanceof Error ? reason.message : String(reason);
        });
    }),
  );
  /** Load a linked reader's Code Review tab from its own project for its own ticket (HS2-9Q4AFB). */
  async function refreshLinkedCodeReview(frameId: string) {
    const frame = linkedReaderStack.value.find((item) => item.id === frameId);
    if (!frame || frame.codeReview?.loading) return;
    const setReview = (codeReview: TicketReaderFrame['codeReview']) => {
      replaceLinkedReaderFrame(frameId, (current) => ({ ...current, codeReview }));
    };
    setReview({ review: frame.codeReview?.review, loading: true, message: '' });
    try {
      setReview({
        review: await new Api(frame.apiPath).codeReview(frame.projectId, frame.ticket.qualified_id),
        loading: false,
        message: '',
      });
    } catch (reason) {
      setReview({ loading: false, message: reason instanceof Error ? reason.message : String(reason) });
    }
  }
  /** Open a linked reader's commit in the diff tool against that reader's project (HS2-9Q4AFB). */
  function openLinkedCodeReview(frame: TicketReaderFrame, target: Element) {
    const reviewTarget = codeReviewTarget(data(target));
    if (!reviewTarget) return;
    const setMessage = (message: string) => {
      replaceLinkedReaderFrame(frame.id, (current) => ({
        ...current,
        codeReview: { ...(current.codeReview ?? { loading: false }), message },
      }));
    };
    setMessage('Opening diff tool…');
    void new Api(frame.apiPath)
      .openCodeReview(frame.projectId, frame.ticket.qualified_id, reviewTarget)
      .then(() => {
        setMessage('');
        showToast(`Opened in ${frame.codeReview?.review?.difftool ?? 'the configured diff tool'}.`);
      })
      .catch((reason: unknown) => {
        if (reason instanceof HostOnlyActionError) {
          setMessage('');
          showToast(reason.message);
        } else setMessage(reason instanceof Error ? reason.message : String(reason));
      });
  }
  return () => {
    lifetime.dispose();
  };
}
