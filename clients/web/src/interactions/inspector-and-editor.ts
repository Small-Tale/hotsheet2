import { delegate, delegateCapture, type Signal } from 'kerfjs';

import { Api, type CodeReview, type FullTicket } from '../api';
import { boardColumnStatus, isPerColumnBoardView } from '../board-pagination';
import { codeReviewTarget } from '../components/ticket-code-review';
import { type InspectorTab } from '../components/ticket-inspector';
import { type TicketReaderDialogElement } from '../components/ticket-reader';
import { addTicketTag, removeTicketTag } from '../components/ticket-tag-editor';
import { type WorkspaceViewMode } from '../components/workspace-header';
import { type DebouncedAutosave } from '../debounced-autosave';
import { parseFeedbackChoices, updateFeedbackChoiceSelection } from '../feedback-choices';
import { DETAILS_FEEDBACK_ID } from '../feedback-needed';
import { combineFeedbackReply, type InlineFeedbackReply, sourceOffsetForVisibleOffset } from '../feedback-replies';
import { manuallyResizedTicketEditorHeight, saveTicketEditorSize, ticketEditorKind } from '../ticket-editor-size';
import { type TicketFieldConflict } from '../ticket-field-reconciliation';
import { type TicketPatch } from '../ticket-operations';
import { type TicketReaderFrame } from '../ticket-reader-stack';
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
  readonly titleEditing: Signal<boolean>;
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
    titleEditing,
    activeTicketSurface,
    titleAutosave,
    tagsAutosave,
    beginDetailsEdit,
    linkedReaderFrame,
    replaceLinkedReaderFrame,
    linkedReaderSaves,
    readerDetailsAutosave,
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
  delegate(document.body, 'click', '[data-action="toggle-inspector-up-next"]', () => {
    if (selectedTicket.value) void updateSelectedTracked({ up_next: !selectedTicket.value.up_next });
  });
  delegate(document.body, 'click', '[data-action="copy-ticket-slug"]', (_event, target) => {
    const slug = target.closest<HTMLElement>('[data-ticket-slug]')?.dataset.ticketSlug;
    if (!slug) return;
    void navigator.clipboard
      .writeText(slug)
      .then(() => {
        showToast(`${slug} copied to clipboard.`);
      })
      .catch((reason: unknown) => {
        error.value = `Copy failed: ${reason instanceof Error ? reason.message : String(reason)}`;
      });
  });
  delegate(document.body, 'change', '[name="inspector-category"]', (_event, target) => {
    void updateSelectedTracked({ category: (target as Control).value });
  });
  delegate(document.body, 'change', '[name="inspector-priority"]', (_event, target) => {
    void updateSelectedTracked({ priority: (target as Control).value });
  });
  delegate(document.body, 'change', '[name="inspector-status"]', (_event, target) => {
    const select = target as Control & { open?: boolean },
      value = select.value,
      apply = () => {
        void updateSelectedTracked({ status: value });
      };
    if (select.open) select.addEventListener('wa-after-hide', apply, { once: true });
    else apply();
  });
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
  delegate(document.body, 'input', '[name="ticket-conflict-resolution"]', (_event, target) => {
    fieldConflictResolution.value = (target as HTMLTextAreaElement).value;
  });
  delegate(document.body, 'click', '[data-action="accept-remote-ticket-field"]', () => {
    const conflict = fieldConflict.value;
    if (!conflict) return;
    updateConflictDraft(conflict.field, conflict.theirs);
    fieldConflict.value = undefined;
    fieldConflictResolution.value = '';
  });
  delegate(document.body, 'click', '[data-action="apply-ticket-field-merge"]', () => {
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
  });
  function beginTitleEdit() {
    if (!selectedTicket.value || !canUpdateSelected()) return;
    titleDraft.value = selectedTicket.value.title;
    dependencies.titleDraftBase = selectedTicket.value.title;
    titleEditing.value = true;
    queueMicrotask(() => activeTicketSurface().querySelector<HTMLElement>('[name="ticket-title"]')?.focus());
  }
  delegate(document.body, 'dblclick', '[data-action="edit-ticket-title"]', () => {
    beginTitleEdit();
  });
  delegate(document.body, 'keydown', '[data-action="edit-ticket-title"]', (event) => {
    if (!['Enter', ' '].includes((event as KeyboardEvent).key)) return;
    event.preventDefault();
    beginTitleEdit();
  });
  delegate(document.body, 'input', '[name="ticket-title"]', (_event, target) => {
    titleDraft.value = (target as HTMLInputElement).value;
    if (titleDraft.value.trim()) titleAutosave.schedule(titleDraft.value);
  });
  delegate(document.body, 'focusout', '[name="ticket-title"]', () => {
    if (!titleDraft.value.trim()) return;
    void titleAutosave.flush().then((saved) => {
      if (saved) titleEditing.value = false;
    });
  });
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
  delegate(document.body, 'keydown', '[name="ticket-tag-input"]', (event, target) => {
    const keyboard = event as KeyboardEvent;
    if (!['Enter', ','].includes(keyboard.key)) return;
    event.preventDefault();
    addTagFromInput(target as HTMLInputElement);
  });
  delegate(document.body, 'focusout', '[name="ticket-tag-input"]', (_event, target) => {
    if ((target as HTMLInputElement).value.trim()) addTagFromInput(target as HTMLInputElement);
    void tagsAutosave.flush();
  });
  delegate(document.body, 'wa-remove', '[data-component="tag-chip"]', (_event, target) => {
    const tag = data(target).tagId;
    if (tag) setSelectedTags(removeTicketTag(selectedTicket.value?.tags ?? [], tag));
  });
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
  });
  delegate(document.body, 'dblclick', '[data-action="edit-markdown"]', (_event, target) => {
    beginDetailsEdit(isReaderSurface(target), linkedReaderFrame(target));
  });
  delegate(document.body, 'click', '[data-action="edit-markdown"]', (_event, target) => {
    if (data(target).empty === 'true') beginDetailsEdit(isReaderSurface(target), linkedReaderFrame(target));
  });
  delegate(document.body, 'keydown', '[data-action="edit-markdown"]', (event, target) => {
    const keyboard = event as KeyboardEvent;
    if (!['Enter', ' '].includes(keyboard.key)) return;
    event.preventDefault();
    beginDetailsEdit(isReaderSurface(target), linkedReaderFrame(target));
  });
  delegate(document.body, 'input', '[name="markdown-source"]', (_event, target) => {
    const frame = linkedReaderFrame(target),
      value = (target as HTMLTextAreaElement).value;
    if (frame) {
      replaceLinkedReaderFrame(frame.id, (current) => ({ ...current, edit: { ...current.edit, detailsDraft: value } }));
      linkedReaderSaves(frame.id).details.schedule(value);
      return;
    }
    const reader = isReaderSurface(target),
      draft = reader ? readerDetailsDraft : detailsDraft,
      autosave = reader ? readerDetailsAutosave : detailsAutosave;
    draft.value = value;
    autosave.schedule(draft.value);
  });
  delegate(document.body, 'focusout', '[name="markdown-source"]', (event, target) => {
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
  });
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
    draft.value = reader && note.kind === 'feedback_needed' ? '' : note.text;
    if (reader) dependencies.readerNoteDraftBase = draft.value;
    else dependencies.noteDraftBase = draft.value;
    queueMicrotask(() =>
      activeTicketSurface().querySelector<HTMLElement>(`[name="note-body"][data-note-id="${id}"]`)?.focus(),
    );
  }
  delegate(document.body, 'click', '[data-action="add-ticket-note"]', () => {
    if (!canAddNotes()) return;
    editingNoteId.value = undefined;
    composingNote.value = true;
    newNoteDraft.value = '';
    scheduleProjectSessionPersistence();
    queueMicrotask(() => activeTicketSurface().querySelector<HTMLElement>('[name="new-note-body"]')?.focus());
  });
  delegate(document.body, 'input', '[name="new-note-body"]', (_event, target) => {
    newNoteDraft.value = (target as HTMLTextAreaElement).value;
    scheduleProjectSessionPersistence();
  });
  delegate(document.body, 'click', '[data-action="cancel-new-note"]', () => {
    composingNote.value = false;
    newNoteDraft.value = '';
    scheduleProjectSessionPersistence();
  });
  delegate(document.body, 'submit', '[data-action="create-note-form"]', (event) => {
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
  });
  delegate(document.body, 'dblclick', '[data-edit-on-double-click="true"]', (_event, target) => {
    beginNoteEdit(data(target.closest('[data-note-id]')!).noteId!, isReaderSurface(target), linkedReaderFrame(target));
  });
  delegate(document.body, 'keydown', '[data-edit-on-double-click="true"]', (event, target) => {
    if (!['Enter', ' '].includes((event as KeyboardEvent).key)) return;
    event.preventDefault();
    beginNoteEdit(data(target.closest('[data-note-id]')!).noteId!, isReaderSurface(target));
  });
  delegate(document.body, 'input', '[name="note-body"]', (_event, target) => {
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
  });
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
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  function inlineFeedbackClickOffset(event:MouseEvent,target:Element){const start=Number(data(target).segmentStart),end=Number(data(target).segmentEnd),noteId=data(target).noteId!,source=feedbackSource(noteId)?.slice(start,end)??'',caretDocument=document as Document&{caretPositionFromPoint?:(x:number,y:number)=>{offsetNode:Node;offset:number}|null},position=caretDocument.caretPositionFromPoint?.(event.clientX,event.clientY);if(!position||!target.contains(position.offsetNode))return end;const range=document.createRange();range.selectNodeContents(target);range.setEnd(position.offsetNode,position.offset);return start+sourceOffsetForVisibleOffset(source,target.textContent??'',range.toString().length)}
  delegate(document.body, 'click', '[data-action="add-inline-feedback-reply"]', (event, target) => {
    if ((event.target as Element).closest('a')) return;
    focusInlineFeedbackReply(data(target).noteId!, inlineFeedbackClickOffset(event as MouseEvent, target));
  });
  delegate(document.body, 'keydown', '[data-action="add-inline-feedback-reply"]', (event, target) => {
    if ((event.target as Element).closest('a') || !['Enter', ' '].includes((event as KeyboardEvent).key)) return;
    event.preventDefault();
    focusInlineFeedbackReply(data(target).noteId!, Number(data(target).segmentEnd));
  });
  delegate(document.body, 'input', '[name="inline-feedback-response"]', (_event, target) => {
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
  });
  delegate(document.body, 'click', '[data-action="remove-inline-feedback-reply"]', (_event, target) => {
    const noteId = data(target).noteId!,
      offset = Number(data(target).offset),
      existing = readerInlineFeedbackReplies.value[noteId] ?? [];
    readerInlineFeedbackReplies.value = {
      ...readerInlineFeedbackReplies.value,
      [noteId]: existing.filter((reply) => reply.offset !== offset),
    };
    scheduleProjectSessionPersistence();
  });
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
  delegate(document.body, 'click', '[data-action="toggle-feedback-choice"]', (event, target) => {
    if ((event.target as Element).closest('a,[data-action="open-attachment-gallery"]')) return;
    toggleFeedbackChoice(event as MouseEvent, target);
  });
  delegate(document.body, 'keydown', '[data-action="toggle-feedback-choice"]', (event, target) => {
    if (
      !['Enter', ' '].includes((event as KeyboardEvent).key) ||
      (event.target as Element).closest('a,[data-action="open-attachment-gallery"]')
    )
      return;
    event.preventDefault();
    toggleFeedbackChoice(event as KeyboardEvent, target);
  });
  delegate(document.body, 'focusout', '[name="note-body"]', (_event, target) => {
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
  });
  delegate(document.body, 'click', '[data-action="save-note-edit"]', (_event, target) => {
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
  });
  delegate(document.body, 'click', '[data-action="dismiss-feedback"]', (_event, target) => {
    const id = data(target).noteId;
    if (!id) return;
    void updateSelected({ note: 'No response needed', note_kind: 'regular' }).then((saved) => {
      if (saved) {
        readerInlineFeedbackReplies.value = { ...readerInlineFeedbackReplies.value, [id]: [] };
        readerFeedbackChoiceSelections.value = { ...readerFeedbackChoiceSelections.value, [id]: [] };
        readerFeedbackChoiceAnchors.delete(id);
      }
    });
  });
  delegate(document.body, 'click', '[data-action="delete-note"]', (_event, target) => {
    const current = project(),
      ticket = selectedTicket.value,
      noteId = data(target).noteId;
    if (!current || !ticket || !noteId || !canDeleteNotes()) return;
    void api()
      .deleteCheckoutNote(current.id, ticket.id, noteId)
      .then((result) => {
        selectedTicket.value = result.ticket;
        editingNoteId.value = undefined;
        noteDraft.value = '';
        return refreshProject();
      })
      .catch((reason: unknown) => {
        error.value = reason instanceof Error ? reason.message : String(reason);
      });
  });
  delegate(document.body, 'click', '[data-action="load-next-ticket-page"]', (_event, target) => {
    const columnId = target.closest<HTMLElement>('[data-component="ticket-board-column"]')?.dataset.columnId;
    if (
      columnId &&
      viewMode.value === 'board' &&
      isPerColumnBoardView(selectedView.value, workspaceSearchActive()) &&
      boardColumnStatus(columnId)
    )
      void loadBoardColumnMore(columnId);
    else void loadNextTicketPage();
  });
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
    draft.value = selectedTicket.value?.blocked_reason ?? '';
    if (reader) dependencies.readerBlockedReasonDraftBase = draft.value;
    else dependencies.blockedReasonDraftBase = draft.value;
    editing.value = true;
    queueMicrotask(() => activeTicketSurface().querySelector<HTMLElement>('[name="blocked-reason"]')?.focus());
  }
  delegate(document.body, 'click', '[data-action="edit-blocked-reason"]', (_event, target) => {
    beginBlockedReasonEdit(isReaderSurface(target), linkedReaderFrame(target));
  });
  delegate(document.body, 'dblclick', '[data-edit-blocked-reason="true"]', (_event, target) => {
    beginBlockedReasonEdit(isReaderSurface(target), linkedReaderFrame(target));
  });
  delegate(document.body, 'keydown', '[data-edit-blocked-reason="true"]', (event, target) => {
    if (!['Enter', ' '].includes((event as KeyboardEvent).key)) return;
    event.preventDefault();
    beginBlockedReasonEdit(isReaderSurface(target));
  });
  delegate(document.body, 'input', '[name="blocked-reason"]', (_event, target) => {
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
  });
  delegate(document.body, 'focusout', '[name="blocked-reason"]', (_event, target) => {
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
  });
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
  );
  delegate(document.body, 'click', '[data-action="open-ticket-reader"]', (_event, target) => {
    presentTicketReaderDialog('workspace-reader', target as HTMLElement, () => {
      readerOpen.value = true;
      if (readerTab.value === 'code-review' && !codeReviewLoading.value) void refreshCodeReview();
    });
  });
  delegate(document.body, 'click', '[data-action="respond-to-feedback"]', (_event, target) => {
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
  });
  delegateCapture(document.body, 'wa-hide', '[data-component="ticket-reader"]', (event, target) => {
    if (event.target !== target) return;
    const dialog = target as TicketReaderDialogElement,
      id = dialog.dataset.readerFrameId;
    if (!id) return;
    if (readerApprovedClose.delete(id)) return;
    event.preventDefault();
    approveTicketReaderClose(dialog);
  });
  delegateCapture(document.body, 'wa-after-hide', '[data-component="ticket-reader"]', (event, target) => {
    if (event.target === target) finishTicketReaderClose(target as TicketReaderDialogElement);
  });
  delegate(document.body, 'click', '[data-action="toggle-reader-text-size"]', () => {
    readerLargeText.value = !readerLargeText.value;
    localStorage.setItem('hotsheet.reader.large-text', String(readerLargeText.value));
  });
  delegate(document.body, 'click', '[data-action="set-inspector-tab"]', (_event, target) => {
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
  });
  delegate(document.body, 'click', '[data-action="open-code-review"]', (_event, target) => {
    const current = project(),
      ticket = selectedTicket.value,
      reviewTarget = codeReviewTarget(data(target));
    if (!current || !ticket || !reviewTarget) return;
    codeReviewMessage.value = 'Opening diff tool…';
    void new Api(current.apiPath)
      .openCodeReview(current.id, ticket.id, reviewTarget)
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
  });
}
