import type { Capabilities, CodeReview, FullTicket } from './api';
import type { InspectorTab } from './components/ticket-inspector';
import type { InlineFeedbackReply } from './feedback-replies';

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
    note = edit.editingNoteId ? ticket.notes.find((item) => item.id === edit.editingNoteId) : undefined;
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
      noteBase: note?.text ?? '',
      noteDraft: edit.editingNoteId && edit.noteDraft !== edit.noteBase ? edit.noteDraft : (note?.text ?? ''),
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
  NOTE_EDIT_FIELDS = new Set(['note', 'note_id', 'edit_feedback_prompt']);

/**
 * Whether a linked reader's provider accepts `patch`: adding a note needs `notes`, editing one needs
 * `note_edit`, and every other field needs `update` (HS2-ANRXAE).
 */
export function linkedReaderPatchAllowed(capabilities: Capabilities, patch: Record<string, unknown>): boolean {
  const keys = Object.keys(patch).filter((key) => key !== 'expected_token');
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
    inlineFeedbackReplies: { ...edit.inlineFeedbackReplies, [noteId]: [] },
    feedbackChoiceSelections: { ...edit.feedbackChoiceSelections, [noteId]: [] },
  };
}
