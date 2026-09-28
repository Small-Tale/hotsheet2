import type { Capabilities, FullTicket } from './api';
import type { InspectorTab } from './components/ticket-inspector';

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
