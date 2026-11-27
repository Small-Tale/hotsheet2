import type { Capabilities, CodeReview, FullTicket } from './api';
import type { InspectorTab } from './components/ticket-inspector';
import type { InlineFeedbackReply } from './feedback-replies';
import type { TicketFieldConflict } from './ticket-field-reconciliation';

export interface TicketReaderFrame {
  id: string;
  open: boolean;
  projectId: string;
  projectName: string;
  apiPath: string;
  ticket: FullTicket;
  activeTab: InspectorTab;
  capabilities: Capabilities;
  edit: TicketReaderEditState;
  /** This frame's own Code Review tab, loaded from its project for its ticket (HS2-9Q4AFB). */
  codeReview?: TicketReaderCodeReviewState;
  /**
   * An overlapping save this frame could not merge, shown as the inspector's conflict card beneath its
   * field until the user chooses Use Theirs or Apply Merged Value (HS2-JT2AFG).
   */
  fieldConflict?: TicketReaderFieldConflict;
}

/** A linked reader's unresolved field conflict and the merged text being edited in its card (HS2-JT2AFG). */
export interface TicketReaderFieldConflict {
  conflict: TicketFieldConflict;
  resolution: string;
}

export interface TicketReaderCodeReviewState {
  review?: CodeReview;
  loading: boolean;
  message: string;
}

export interface TicketReaderEditState {
  detailsMode: 'preview' | 'write';
  detailsDraft: string;
  detailsBase: string;
  detailsGeneration: number;
  editingNoteId?: string;
  /** An append reply must not become a note edit after a refresh (HS2-VBFSZJ). */
  responseRequestId?: string;
  noteDraft: string;
  noteBase: string;
  noteGeneration: number;
  blockedReasonEditing: boolean;
  blockedReasonDraft: string;
  blockedReasonBase: string;
  blockedReasonGeneration: number;
  /** This frame's own new-note composer (HS2-ANRXAE); a new note is a submission, not an autosaved edit. */
  composingNote: boolean;
  newNoteDraft: string;
  /** Inline feedback reply drafts and choice selections, keyed by note id within this frame (HS2-ANRXAE). */
  inlineFeedbackReplies: Record<string, InlineFeedbackReply[]>;
  feedbackChoiceSelections: Record<string, string[]>;
}

export function ticketReaderEditState(ticket: FullTicket): TicketReaderEditState {
  return {
    detailsMode: 'preview',
    detailsDraft: ticket.details,
    detailsBase: ticket.details,
    detailsGeneration: 0,
    noteDraft: '',
    noteBase: '',
    noteGeneration: 0,
    blockedReasonEditing: false,
    blockedReasonDraft: ticket.blocked_reason ?? '',
    blockedReasonBase: ticket.blocked_reason ?? '',
    blockedReasonGeneration: 0,
    composingNote: false,
    newNoteDraft: '',
    inlineFeedbackReplies: {},
    feedbackChoiceSelections: {},
  };
}

export function updateTicketReaderFrame(
  stack: readonly TicketReaderFrame[],
  id: string,
  update: (frame: TicketReaderFrame) => TicketReaderFrame,
): TicketReaderFrame[] {
  return stack.map((frame) => (frame.id === id ? update(frame) : frame));
}

export function reconcileTicketReaderFrame(frame: TicketReaderFrame, ticket: FullTicket): TicketReaderFrame {
  const edit = frame.edit,
    note = edit.editingNoteId ? ticket.notes.find((item) => item.id === edit.editingNoteId) : undefined,
    response = Boolean(
      edit.responseRequestId &&
      edit.responseRequestId === edit.editingNoteId &&
      frame.ticket.qualified_id === ticket.qualified_id,
    );
  return {
    ...frame,
    ticket,
    edit: {
      ...edit,
      detailsBase: ticket.details,
      detailsDraft:
        edit.detailsMode === 'write' && edit.detailsDraft !== edit.detailsBase ? edit.detailsDraft : ticket.details,
      blockedReasonBase: ticket.blocked_reason ?? '',
      blockedReasonDraft:
        edit.blockedReasonEditing && edit.blockedReasonDraft !== edit.blockedReasonBase
          ? edit.blockedReasonDraft
          : (ticket.blocked_reason ?? ''),
      noteBase: response ? edit.noteBase : (note?.text ?? ''),
      noteDraft:
        response || (edit.editingNoteId && edit.noteDraft !== edit.noteBase) ? edit.noteDraft : (note?.text ?? ''),
    },
  };
}

/**
 * After a save committed `committed`, drafts that still hold exactly what was `sent` adopt the committed
 * text, which differs when the save merged a concurrent edit (HS2-A4XCXE). Later typing then builds on the
 * merged text instead of overwriting the other edit.
 */
export function adoptCommittedReaderDrafts(
  frame: TicketReaderFrame,
  sent: Record<string, unknown>,
  committed: FullTicket,
): TicketReaderFrame {
  const edit = { ...frame.edit },
    text = (value: unknown) => (typeof value === 'string' ? value : '');
  if (typeof sent.details === 'string' && edit.detailsMode === 'write' && edit.detailsDraft === sent.details)
    edit.detailsDraft = edit.detailsBase = committed.details;
  if (
    Object.hasOwn(sent, 'blocked_reason') &&
    edit.blockedReasonEditing &&
    edit.blockedReasonDraft.trim() === text(sent.blocked_reason).trim()
  )
    edit.blockedReasonDraft = edit.blockedReasonBase = committed.blocked_reason ?? '';
  if (typeof sent.note === 'string' && sent.note_id === edit.editingNoteId && edit.noteDraft === sent.note) {
    const note = committed.notes.find((item) => item.id === edit.editingNoteId);
    if (note) edit.noteDraft = edit.noteBase = note.text;
  }
  return { ...frame, edit };
}

export function pushTicketReaderFrame(
  stack: readonly TicketReaderFrame[],
  frame: TicketReaderFrame,
): TicketReaderFrame[] {
  return [...stack, frame];
}

export function popTicketReaderFrame(stack: readonly TicketReaderFrame[]): {
  stack: TicketReaderFrame[];
  closed?: TicketReaderFrame;
} {
  if (stack.length === 0) return { stack: [] };
  return { stack: stack.slice(0, -1), closed: stack.at(-1) };
}

export function activeTicketReaderProject(stack: readonly TicketReaderFrame[], fallbackProjectId: string): string {
  return stack.at(-1)?.projectId ?? fallbackProjectId;
}

export function disposeTicketReaderFrames(
  stack: readonly TicketReaderFrame[],
  projectIds: ReadonlySet<string>,
): { retained: TicketReaderFrame[]; disposed: TicketReaderFrame[] } {
  const retained: TicketReaderFrame[] = [],
    disposed: TicketReaderFrame[] = [];
  for (const frame of stack) (projectIds.has(frame.projectId) ? disposed : retained).push(frame);
  return { retained, disposed };
}

/**
 * The ticket field edits apply to: the top linked reader when one is stacked, otherwise the workspace
 * selection. A link-opened reader is editable exactly like any other reader, gated only by its own
 * provider's update capability (HS2-KRVWKF).
 */
export function ticketEditTarget(
  stack: readonly TicketReaderFrame[],
  selected: FullTicket | null,
  canUpdateSelected: () => boolean,
): { ticket: FullTicket | null; canUpdate: boolean; frame?: TicketReaderFrame } {
  const top = stack.at(-1);
  return top
    ? { ticket: top.ticket, canUpdate: top.capabilities.update, frame: top }
    : { ticket: selected, canUpdate: Boolean(selected) && canUpdateSelected() };
}

/** Whether a linked reader layer offers field editing: only the interactive top layer does (HS2-KRVWKF). */
export function linkedReaderCanUpdate(frame: TicketReaderFrame, active: boolean): boolean {
  return active && frame.capabilities.update;
}

const NOTE_ADD_FIELDS = new Set(['note', 'note_kind']),
  NOTE_EDIT_FIELDS = new Set(['note', 'note_id', 'note_kind', 'edit_feedback_prompt', 'response_to']);

/**
 * Whether a linked reader's provider accepts `patch`: adding a note needs `notes`, editing one needs
 * `note_edit`, and every other field needs `update` (HS2-ANRXAE).
 */
export function linkedReaderPatchAllowed(capabilities: Capabilities, patch: Record<string, unknown>): boolean {
  const keys = Object.keys(patch).filter((key) => key !== 'expected_token');
  // HS2-Y32T8A: Up Next is its own reported capability (GitLab and Jira refuse it).
  if (keys.includes('up_next') && !capabilities.up_next) return false;
  if (keys.includes('note_id') && keys.includes('note_kind') && !capabilities.note_kind_edit) return false;
  // HS2-J4G9GB: linking an existing note as a response is its own reported capability.
  if (keys.includes('note_id') && keys.includes('response_to') && !capabilities.note_response_link) return false;
  if (keys.length > 0 && keys.every((key) => NOTE_ADD_FIELDS.has(key))) return capabilities.notes;
  if (keys.includes('note_id') && keys.every((key) => NOTE_EDIT_FIELDS.has(key))) return capabilities.note_edit;
  return capabilities.update;
}

/** The started phase a menu item selects, moving a ticket that is not yet Started to Started (HS2-ANRXAE). */
export function startedPhasePatch(ticket: FullTicket | null | undefined, phase: string): Record<string, unknown> {
  const patch: Record<string, unknown> = { started_phase: phase || null };
  if (ticket?.status !== 'started') patch.status = 'started';
  return patch;
}

/** Remove one frame's state for a note's feedback reply after it was answered (HS2-ANRXAE). */
export function clearFrameFeedbackReply(edit: TicketReaderEditState, noteId: string): TicketReaderEditState {
  return {
    ...edit,
    ...(edit.editingNoteId === noteId ? { editingNoteId: undefined, noteDraft: '', noteBase: '' } : {}),
    responseRequestId: edit.responseRequestId === noteId ? undefined : edit.responseRequestId,
    inlineFeedbackReplies: { ...edit.inlineFeedbackReplies, [noteId]: [] },
    feedbackChoiceSelections: { ...edit.feedbackChoiceSelections, [noteId]: [] },
  };
}

/** Show `conflict` in the frame's card, its merge starting from the user's own text (HS2-JT2AFG). */
export function showReaderFieldConflict(frame: TicketReaderFrame, conflict: TicketFieldConflict): TicketReaderFrame {
  return { ...frame, fieldConflict: { conflict, resolution: conflict.mine } };
}

/** The merged text the user is editing in the frame's conflict card (HS2-JT2AFG). */
export function editReaderFieldConflict(frame: TicketReaderFrame, resolution: string): TicketReaderFrame {
  return frame.fieldConflict ? { ...frame, fieldConflict: { ...frame.fieldConflict, resolution } } : frame;
}

/**
 * Drop the frame's conflict card once a newer save of the same field starts, because the user typed
 * into that field again (HS2-Y0VRXR, HS2-JT2AFG). A conflict on another field stays.
 */
export function settleReaderFieldConflict(frame: TicketReaderFrame, keys: readonly string[]): TicketReaderFrame {
  if (!frame.fieldConflict || !keys.includes(frame.fieldConflict.conflict.key)) return frame;
  return { ...frame, fieldConflict: undefined };
}

/**
 * Resolve the frame's conflict card with `value` (their text for Use Theirs, the merged text for Apply
 * Merged Value): the field's open editor takes `value` on top of `base`, the remote text it now holds,
 * so the next focus loss sends nothing unless `value` still differs from the ticket (HS2-JT2AFG).
 */
export function resolveReaderFieldConflict(frame: TicketReaderFrame, value: string, base: string): TicketReaderFrame {
  const conflict = frame.fieldConflict?.conflict;
  if (!conflict) return frame;
  const edit = { ...frame.edit };
  if (conflict.field === 'details' && edit.detailsMode === 'write') {
    edit.detailsDraft = value;
    edit.detailsBase = base;
  } else if (conflict.field === 'blocked_reason' && edit.blockedReasonEditing) {
    edit.blockedReasonDraft = value;
    edit.blockedReasonBase = base;
  } else if (conflict.field === 'note' && edit.editingNoteId === conflict.key.slice('note:'.length)) {
    edit.noteDraft = value;
    edit.noteBase = base;
  }
  return { ...frame, edit, fieldConflict: undefined };
}
