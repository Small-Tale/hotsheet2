import { delegate, delegateCapture, type Signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { Api, type CodeReview, type FullTicket } from '../api';
import { boardColumnStatus, isPerColumnBoardView } from '../board-pagination';
import { TAG_CHIP_REMOVE_ACTION } from '../components/tag-chip';
import { codeReviewTarget } from '../components/ticket-code-review';
import { type InspectorTab } from '../components/ticket-inspector';
import { type TicketReaderDialogElement } from '../components/ticket-reader';
import { statusMenuAnchorY } from '../components/ticket-status-menu';
import { addTicketTag, removeTicketTag } from '../components/ticket-tag-editor';
import { type WorkspaceViewMode } from '../components/workspace-header';
import { maintainContextPopupMenuAnchor } from '../context-menu-position';
import { copyText } from '../copy-text';
import { type DebouncedAutosave } from '../debounced-autosave';
import { parseFeedbackChoices, updateFeedbackChoiceSelection } from '../feedback-choices';
import { DETAILS_FEEDBACK_ID } from '../feedback-needed';
import { combineFeedbackReply, type InlineFeedbackReply, sourceOffsetForVisibleOffset } from '../feedback-replies';
import { inlineEditorEscape } from '../inline-editor-escape';
import { ATTACHMENTS_AND_GALLERY_TARGETS } from '../interaction-attrs/attachments-and-gallery';
import { INSPECTOR_AND_EDITOR_ACTIONS, INSPECTOR_AND_EDITOR_TARGETS } from '../interaction-attrs/inspector-and-editor';
import { clickBeginsMarkdownEdit, keyBeginsMarkdownEdit, repeatPressWouldLeaveNewEditor } from '../markdown-click-edit';
import { manuallyResizedTicketEditorHeight, saveTicketEditorSize, ticketEditorKind } from '../ticket-editor-size';
import { type TicketFieldConflict } from '../ticket-field-reconciliation';
import { type TicketPatch } from '../ticket-operations';
import { type TicketReaderFrame } from '../ticket-reader-stack';
import {
  normalizeTicketTitleField,
  type TicketTitleEditSurface,
  ticketTitleEditSurfaceOf,
  ticketTitleKeyFinishesEdit,
} from '../ticket-title-editing';
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
  readonly readerDialog: (id: string) => TicketReaderDialogElement | null;
  readonly readerApprovedClose: Set<string>;
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
    readerDialog,
    readerApprovedClose,
    approveTicketReaderClose,
    finishTicketReaderClose,
    readerLargeText,
    linkedReaderStack,
    inspectorTab,
    codeReviewMessage,
    codeReview,
  } = dependencies;
  let stopStatusMenuAnchor: (() => void) | undefined;
  lifetime.add(() => stopStatusMenuAnchor?.());
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.toggleInspectorUpNext.selector, () => {
      if (selectedTicket.value) void updateSelectedTracked({ up_next: !selectedTicket.value.up_next });
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
    delegate(
      document.body,
      'click',
      INSPECTOR_AND_EDITOR_ACTIONS.openInspectorStatusMenu.selector,
      (_event, target) => {
        const root = target.closest('#app-right-rail, [data-component="ticket-reader"]');
        if (!root?.querySelector('[data-inspector-status-menu]')) return;
        const rect = target.getBoundingClientRect();
        const ticketId = selectedTicket.peek()?.id;
        stopStatusMenuAnchor?.();
        stopStatusMenuAnchor = maintainContextPopupMenuAnchor(
          '[data-inspector-status-menu]',
          { x: rect.left, y: statusMenuAnchorY(rect.bottom, window.innerWidth, window.innerHeight) },
          () => root.isConnected && selectedTicket.peek()?.id === ticketId,
          root,
        );
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.setInspectorStatus.selector, (_event, target) => {
      if (!canUpdateSelected()) return;
      const status = (target as HTMLElement).dataset.ticketStatus;
      if (status) void updateSelectedTracked({ status });
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      INSPECTOR_AND_EDITOR_ACTIONS.setInspectorStartedPhase.selector,
      (_event, target) => {
        if (!canEditStartedPhaseSelected()) return;
        const phase = (target as HTMLElement).dataset.startedPhase;
        if (phase === undefined) return;
        const patch: TicketPatch = { started_phase: phase || null };
        if (selectedTicket.value?.status !== 'started') patch.status = 'started';
        void updateSelectedTracked(patch);
      },
    ),
  );
  function updateConflictDraft(field: string, value: string, base = value) {
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
    if (conflict.field === 'note') return { note_id: conflict.key.slice('note:'.length), note: value };
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
        fieldConflictResolution.value = (target as HTMLTextAreaElement).value;
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.acceptRemoteTicketField.selector, () => {
      const conflict = fieldConflict.value;
      if (!conflict) return;
      updateConflictDraft(conflict.field, conflict.theirs);
      fieldConflict.value = undefined;
      fieldConflictResolution.value = '';
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.applyTicketFieldMerge.selector, () => {
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
    if (!selectedTicket.value || !canUpdateSelected()) return;
    const restoredTitle = restoreTicketDraft('title', selectedTicket.value.title);
    titleDraft.value = restoredTitle?.draft ?? selectedTicket.value.title;
    dependencies.titleDraftBase = restoredTitle?.base ?? selectedTicket.value.title;
    // Only the surface that started the edit shows the editor; the other keeps its heading (HS2-2M5BBN).
    titleEditingSurface.value = ticketTitleEditSurfaceOf(trigger);
    if (restoredTitle) titleAutosave.schedule(titleDraft.value);
    queueMicrotask(() => activeTicketSurface().querySelector<HTMLElement>('[name="ticket-title"]')?.focus());
  }
  lifetime.add(
    delegate(document.body, 'dblclick', INSPECTOR_AND_EDITOR_ACTIONS.editTicketTitle.selector, (_event, target) => {
      beginTitleEdit(target);
    }),
  );
  lifetime.add(
    delegate(document.body, 'keydown', INSPECTOR_AND_EDITOR_ACTIONS.editTicketTitle.selector, (event, target) => {
      if (!['Enter', ' '].includes((event as KeyboardEvent).key)) return;
      event.preventDefault();
      beginTitleEdit(target);
    }),
  );
  lifetime.add(
    delegate(document.body, 'input', INSPECTOR_AND_EDITOR_TARGETS.ticketTitleField.selector, (_event, target) => {
      // The wrapping editor is a textarea, but a title stays one line: pasted breaks collapse (HS2-98ZVPE).
      titleDraft.value = normalizeTicketTitleField(target as HTMLTextAreaElement);
      if (titleDraft.value.trim()) titleAutosave.schedule(titleDraft.value);
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
  lifetime.add(
    delegate(document.body, 'keydown', INSPECTOR_AND_EDITOR_TARGETS.ticketTitleField.selector, (event, target) => {
      // Enter finishes the edit through the focusout autosave instead of inserting a newline.
      if (!ticketTitleKeyFinishesEdit(event as KeyboardEvent)) return;
      event.preventDefault();
      (target as HTMLTextAreaElement).blur();
    }),
  );
  lifetime.add(
    delegate(document.body, 'focusout', INSPECTOR_AND_EDITOR_TARGETS.ticketTitleField.selector, () => {
      if (!titleDraft.value.trim()) return;
      void titleAutosave.flush().then((saved) => {
        if (saved) titleEditingSurface.value = undefined;
      });
    }),
  );
  function setSelectedTags(tags: string[]) {
    if (!selectedTicket.value || !canUpdateSelected()) return;
    selectedTicket.value = { ...selectedTicket.value, tags };
    tagsAutosave.schedule(tags);
  }
  function addTagFromInput(target: HTMLInputElement) {
    const next = addTicketTag(selectedTicket.value?.tags ?? [], target.value);
    target.value = '';
    setSelectedTags(next);
  }
  lifetime.add(
    delegate(document.body, 'keydown', INSPECTOR_AND_EDITOR_TARGETS.ticketTagInputField.selector, (event, target) => {
      const keyboard = event as KeyboardEvent;
      if (!['Enter', ','].includes(keyboard.key)) return;
      event.preventDefault();
      addTagFromInput(target as HTMLInputElement);
    }),
  );
  lifetime.add(
    delegate(document.body, 'focusout', INSPECTOR_AND_EDITOR_TARGETS.ticketTagInputField.selector, (_event, target) => {
      if ((target as HTMLInputElement).value.trim()) addTagFromInput(target as HTMLInputElement);
      void tagsAutosave.flush();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', `[data-action="${TAG_CHIP_REMOVE_ACTION}"]`, (_event, target) => {
      const chip = target.closest<HTMLElement>('[data-component="tag-chip"]'),
        tag = chip ? data(chip).tagId : undefined;
      if (tag) setSelectedTags(removeTicketTag(selectedTicket.value?.tags ?? [], tag));
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
    delegateCapture(document.body, 'mousedown', '*', (event) => {
      if (repeatPressWouldLeaveNewEditor(event as MouseEvent, document.activeElement)) event.preventDefault();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.editMarkdown.selector, (event, target) => {
      if (clickBeginsMarkdownEdit(event as MouseEvent, target))
        beginDetailsEdit(isReaderSurface(target), linkedReaderFrame(target));
    }),
  );
  lifetime.add(
    delegate(document.body, 'keydown', INSPECTOR_AND_EDITOR_ACTIONS.editMarkdown.selector, (event, target) => {
      if (!keyBeginsMarkdownEdit(event as KeyboardEvent, target)) return;
      event.preventDefault();
      beginDetailsEdit(isReaderSurface(target), linkedReaderFrame(target));
    }),
  );
  lifetime.add(
    delegate(document.body, 'input', INSPECTOR_AND_EDITOR_TARGETS.markdownSourceField.selector, (_event, target) => {
      const frame = linkedReaderFrame(target),
        value = (target as HTMLTextAreaElement).value;
      if (frame) {
        replaceLinkedReaderFrame(frame.id, (current) => ({
          ...current,
          edit: { ...current.edit, detailsDraft: value },
        }));
        linkedReaderSaves(frame.id).details.schedule(value);
        return;
      }
      const reader = isReaderSurface(target),
        draft = reader ? readerDetailsDraft : detailsDraft,
        autosave = reader ? readerDetailsAutosave : detailsAutosave;
      draft.value = value;
      autosave.schedule(draft.value);
    }),
  );
  lifetime.add(
    delegate(document.body, 'focusout', INSPECTOR_AND_EDITOR_TARGETS.markdownSourceField.selector, (event, target) => {
      const frame = linkedReaderFrame(target);
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
      const next = (event as FocusEvent).relatedTarget;
      if (next instanceof Node && target.closest('[data-component="markdown-editor"]')?.contains(next)) {
        dependencies.pointerDetailsReader = undefined;
        return;
      }
      if (next instanceof Element && next.closest('[data-component="ticket-field-conflict"]')) {
        dependencies.pointerDetailsReader = undefined;
        return;
      }
      const reader = isReaderSurface(target);
      if (dependencies.pointerDetailsReader === reader) {
        dependencies.pointerDetailsFinish = beginDetailsFinish(reader);
        return;
      }
      setTimeout(() => void finishDetailsEdit(reader), 0);
    }),
  );
  function beginNoteEdit(id: string, reader = false, frame?: TicketReaderFrame) {
    const note = (frame?.ticket ?? selectedTicket.value)?.notes.find((item) => item.id === id);
    if (!note) return;
    if (frame) {
      if (!frame.capabilities.note_edit) return;
      replaceLinkedReaderFrame(frame.id, (current) => ({
        ...current,
        edit: {
          ...current.edit,
          editingNoteId: id,
          noteDraft: note.text,
          noteBase: note.text,
          noteGeneration: current.edit.noteGeneration + 1,
        },
      }));
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
    const initial = reader && note.kind === 'feedback_needed' ? '' : note.text,
      restoredNote = initial === note.text ? restoreTicketDraft('note', note.text, id) : undefined;
    draft.value = restoredNote?.draft ?? initial;
    if (reader) dependencies.readerNoteDraftBase = restoredNote?.base ?? initial;
    else dependencies.noteDraftBase = restoredNote?.base ?? initial;
    if (restoredNote) (reader ? readerNoteAutosave : noteAutosave).schedule({ id, value: draft.value });
    queueMicrotask(() =>
      activeTicketSurface().querySelector<HTMLElement>(`[name="note-body"][data-note-id="${id}"]`)?.focus(),
    );
  }
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.addTicketNote.selector, () => {
      if (!canAddNotes()) return;
      editingNoteId.value = undefined;
      composingNote.value = true;
      newNoteDraft.value = '';
      scheduleProjectSessionPersistence();
      queueMicrotask(() => activeTicketSurface().querySelector<HTMLElement>('[name="new-note-body"]')?.focus());
    }),
  );
  lifetime.add(
    delegate(document.body, 'input', INSPECTOR_AND_EDITOR_TARGETS.newNoteBodyField.selector, (_event, target) => {
      newNoteDraft.value = (target as HTMLTextAreaElement).value;
      scheduleProjectSessionPersistence();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.cancelNewNote.selector, () => {
      composingNote.value = false;
      newNoteDraft.value = '';
      scheduleProjectSessionPersistence();
    }),
  );
  lifetime.add(
    delegate(document.body, 'submit', INSPECTOR_AND_EDITOR_ACTIONS.createNoteForm.selector, (event) => {
      event.preventDefault();
      const text = newNoteDraft.value.trim();
      if (!text || !canAddNotes()) return;
      void updateSelected({ note: text, note_kind: 'regular' }).then((saved) => {
        if (saved) {
          composingNote.value = false;
          newNoteDraft.value = '';
          scheduleProjectSessionPersistence();
        }
      });
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_TARGETS.editOnClick.selector, (event, target) => {
      if (!clickBeginsMarkdownEdit(event as MouseEvent, target)) return;
      beginNoteEdit(
        data(target.closest('[data-note-id]')!).noteId!,
        isReaderSurface(target),
        linkedReaderFrame(target),
      );
    }),
  );
  lifetime.add(
    delegate(document.body, 'keydown', INSPECTOR_AND_EDITOR_TARGETS.editOnClick.selector, (event, target) => {
      if (!keyBeginsMarkdownEdit(event as KeyboardEvent, target)) return;
      event.preventDefault();
      beginNoteEdit(
        data(target.closest('[data-note-id]')!).noteId!,
        isReaderSurface(target),
        linkedReaderFrame(target),
      );
    }),
  );
  lifetime.add(
    delegate(document.body, 'input', INSPECTOR_AND_EDITOR_TARGETS.noteBodyField.selector, (_event, target) => {
      const frame = linkedReaderFrame(target),
        id = data(target).noteId,
        value = (target as HTMLTextAreaElement).value;
      if (frame && id) {
        replaceLinkedReaderFrame(frame.id, (current) => ({
          ...current,
          edit: { ...current.edit, editingNoteId: id, noteDraft: value },
        }));
        linkedReaderSaves(frame.id).note.schedule({ id, value });
        return;
      }
      const reader = isReaderSurface(target),
        editing = reader ? readerEditingNoteId : editingNoteId,
        draft = reader ? readerNoteDraft : noteDraft,
        autosave = reader ? readerNoteAutosave : noteAutosave;
      editing.value = id;
      draft.value = value;
      if (data(target).noteResponse !== 'true' && editing.value)
        autosave.schedule({ id: editing.value, value: draft.value });
      else scheduleProjectSessionPersistence();
    }),
  );
  function focusInlineFeedbackReply(noteId: string, offset: number) {
    const existing = readerInlineFeedbackReplies.value[noteId] ?? [];
    if (!existing.some((reply) => reply.offset === offset))
      readerInlineFeedbackReplies.value = {
        ...readerInlineFeedbackReplies.value,
        [noteId]: [...existing, { offset, text: '' }],
      };
    scheduleProjectSessionPersistence();
    queueMicrotask(() =>
      activeTicketSurface()
        .querySelector<HTMLElement>(
          `[name="inline-feedback-response"][data-note-id="${noteId}"][data-offset="${offset}"]`,
        )
        ?.focus(),
    );
  }
  function feedbackSource(noteId: string) {
    return noteId === DETAILS_FEEDBACK_ID
      ? selectedTicket.value?.details
      : selectedTicket.value?.notes.find((item) => item.id === noteId)?.text;
  }
  function inlineFeedbackClickOffset(event: MouseEvent, target: Element) {
    const start = Number(data(target).segmentStart),
      end = Number(data(target).segmentEnd),
      noteId = data(target).noteId!,
      source = feedbackSource(noteId)?.slice(start, end) ?? '',
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
      focusInlineFeedbackReply(data(target).noteId!, inlineFeedbackClickOffset(event as MouseEvent, target));
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
        focusInlineFeedbackReply(data(target).noteId!, Number(data(target).segmentEnd));
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'input',
      INSPECTOR_AND_EDITOR_TARGETS.inlineFeedbackResponseField.selector,
      (_event, target) => {
        const noteId = data(target).noteId!,
          offset = Number(data(target).offset),
          existing = readerInlineFeedbackReplies.value[noteId] ?? [];
        readerInlineFeedbackReplies.value = {
          ...readerInlineFeedbackReplies.value,
          [noteId]: existing.map((reply) =>
            reply.offset === offset ? { ...reply, text: (target as HTMLTextAreaElement).value } : reply,
          ),
        };
        scheduleProjectSessionPersistence();
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      INSPECTOR_AND_EDITOR_ACTIONS.removeInlineFeedbackReply.selector,
      (_event, target) => {
        const noteId = data(target).noteId!,
          offset = Number(data(target).offset),
          existing = readerInlineFeedbackReplies.value[noteId] ?? [];
        readerInlineFeedbackReplies.value = {
          ...readerInlineFeedbackReplies.value,
          [noteId]: existing.filter((reply) => reply.offset !== offset),
        };
        scheduleProjectSessionPersistence();
      },
    ),
  );
  function toggleFeedbackChoice(event: MouseEvent | KeyboardEvent, target: Element) {
    const noteId = data(target).noteId!,
      choiceId = data(target).choiceId!,
      source = feedbackSource(noteId),
      group = source && parseFeedbackChoices(source);
    if (!group) return;
    const next = updateFeedbackChoiceSelection(
      group.choices.map((choice) => choice.id),
      readerFeedbackChoiceSelections.value[noteId] ?? [],
      choiceId,
      readerFeedbackChoiceAnchors.get(noteId),
      { additive: event.metaKey || event.ctrlKey, range: event.shiftKey },
    );
    readerFeedbackChoiceSelections.value = { ...readerFeedbackChoiceSelections.value, [noteId]: next.selected };
    if (next.anchor) readerFeedbackChoiceAnchors.set(noteId, next.anchor);
    scheduleProjectSessionPersistence();
  }
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.toggleFeedbackChoice.selector, (event, target) => {
      if ((event.target as Element).closest('a,[data-action="open-attachment-gallery"]')) return;
      toggleFeedbackChoice(event as MouseEvent, target);
    }),
  );
  lifetime.add(
    delegate(document.body, 'keydown', INSPECTOR_AND_EDITOR_ACTIONS.toggleFeedbackChoice.selector, (event, target) => {
      if (
        !['Enter', ' '].includes((event as KeyboardEvent).key) ||
        (event.target as Element).closest('a,[data-action="open-attachment-gallery"]')
      )
        return;
      event.preventDefault();
      toggleFeedbackChoice(event as KeyboardEvent, target);
    }),
  );
  lifetime.add(
    delegate(document.body, 'focusout', INSPECTOR_AND_EDITOR_TARGETS.noteBodyField.selector, (_event, target) => {
      if (data(target).noteResponse === 'true') return;
      const frame = linkedReaderFrame(target);
      if (frame) {
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
      const reader = isReaderSurface(target),
        editing = reader ? readerEditingNoteId : editingNoteId,
        draft = reader ? readerNoteDraft : noteDraft,
        autosave = reader ? readerNoteAutosave : noteAutosave;
      void autosave.flush().then((saved) => {
        if (saved) {
          editing.value = undefined;
          draft.value = '';
        }
      });
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.saveNoteEdit.selector, (_event, target) => {
      const reader = isReaderSurface(target),
        editing = reader ? readerEditingNoteId : editingNoteId,
        draft = reader ? readerNoteDraft : noteDraft,
        id = editing.value ?? data(target).noteId,
        note = selectedTicket.value?.notes.find((item) => item.id === id),
        response = data(target).noteResponse === 'true' || note?.kind === 'feedback_draft',
        source = id ? feedbackSource(id) : undefined,
        value =
          source && response
            ? combineFeedbackReply(
                source,
                readerInlineFeedbackReplies.value[id ?? ''] ?? [],
                draft.value,
                readerFeedbackChoiceSelections.value[id ?? ''] ?? [],
              )
            : draft.value;
      if (!id || !value.trim()) return;
      const patch = response ? { note: value, note_kind: 'regular' } : { note_id: id, note: value };
      void updateSelected(patch).then((saved) => {
        if (saved) {
          editing.value = undefined;
          draft.value = '';
          readerInlineFeedbackReplies.value = { ...readerInlineFeedbackReplies.value, [id]: [] };
          readerFeedbackChoiceSelections.value = { ...readerFeedbackChoiceSelections.value, [id]: [] };
          readerFeedbackChoiceAnchors.delete(id);
        }
      });
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.dismissFeedback.selector, (_event, target) => {
      const id = data(target).noteId;
      if (!id) return;
      void updateSelected({ note: 'No response needed', note_kind: 'regular' }).then((saved) => {
        if (saved) {
          readerInlineFeedbackReplies.value = { ...readerInlineFeedbackReplies.value, [id]: [] };
          readerFeedbackChoiceSelections.value = { ...readerFeedbackChoiceSelections.value, [id]: [] };
          readerFeedbackChoiceAnchors.delete(id);
        }
      });
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.deleteNote.selector, (_event, target) => {
      const current = project(),
        ticket = selectedTicket.value,
        noteId = data(target).noteId;
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
  function beginBlockedReasonEdit(reader = false, frame?: TicketReaderFrame) {
    if (frame) {
      if (!frame.capabilities.update) return;
      replaceLinkedReaderFrame(frame.id, (current) => ({
        ...current,
        edit: {
          ...current.edit,
          blockedReasonEditing: true,
          blockedReasonDraft: current.ticket.blocked_reason ?? '',
          blockedReasonBase: current.ticket.blocked_reason ?? '',
          blockedReasonGeneration: current.edit.blockedReasonGeneration + 1,
        },
      }));
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
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.editBlockedReason.selector, (_event, target) => {
      beginBlockedReasonEdit(isReaderSurface(target), linkedReaderFrame(target));
    }),
  );
  lifetime.add(
    delegate(document.body, 'dblclick', INSPECTOR_AND_EDITOR_TARGETS.editBlockedReason.selector, (_event, target) => {
      beginBlockedReasonEdit(isReaderSurface(target), linkedReaderFrame(target));
    }),
  );
  lifetime.add(
    delegate(document.body, 'keydown', INSPECTOR_AND_EDITOR_TARGETS.editBlockedReason.selector, (event, target) => {
      if (!['Enter', ' '].includes((event as KeyboardEvent).key)) return;
      event.preventDefault();
      beginBlockedReasonEdit(isReaderSurface(target));
    }),
  );
  lifetime.add(
    delegate(document.body, 'input', INSPECTOR_AND_EDITOR_TARGETS.blockedReasonField.selector, (_event, target) => {
      const frame = linkedReaderFrame(target),
        value = (target as HTMLTextAreaElement).value;
      if (frame) {
        replaceLinkedReaderFrame(frame.id, (current) => ({
          ...current,
          edit: { ...current.edit, blockedReasonDraft: value },
        }));
        linkedReaderSaves(frame.id).blocked.schedule(value);
        return;
      }
      const reader = isReaderSurface(target),
        draft = reader ? readerBlockedReasonDraft : blockedReasonDraft,
        autosave = reader ? readerBlockedReasonAutosave : blockedReasonAutosave;
      draft.value = value;
      autosave.schedule(draft.value);
    }),
  );
  lifetime.add(
    delegate(document.body, 'focusout', INSPECTOR_AND_EDITOR_TARGETS.blockedReasonField.selector, (_event, target) => {
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
    delegateCapture(document.body, 'wa-hide', INSPECTOR_AND_EDITOR_TARGETS.ticketReader.selector, (event, target) => {
      if (event.target !== target) return;
      const dialog = target as TicketReaderDialogElement,
        id = dialog.dataset.readerFrameId;
      if (!id) return;
      if (readerApprovedClose.delete(id)) return;
      event.preventDefault();
      approveTicketReaderClose(dialog);
    }),
  );
  lifetime.add(
    delegateCapture(
      document.body,
      'wa-after-hide',
      INSPECTOR_AND_EDITOR_TARGETS.ticketReader.selector,
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
      if ((!frameId || frameId === 'workspace-reader') && tab === 'code-review' && !codeReviewLoading.value)
        void refreshCodeReview();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.openCodeReview.selector, (_event, target) => {
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
          if (project()?.id === current.id && selectedTicket.value?.id === ticket.id)
            codeReviewMessage.value = reason instanceof Error ? reason.message : String(reason);
        });
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
