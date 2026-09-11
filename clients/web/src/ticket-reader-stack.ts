import type { Capabilities, FullTicket } from './api';
import type { InspectorTab } from './components/ticket-inspector';

export interface TicketReaderFrame {
  id: string;
  projectId: string;
  projectName: string;
  apiPath: string;
  ticket: FullTicket;
  activeTab: InspectorTab;
  capabilities: Capabilities;
  edit: TicketReaderEditState;
}

export interface TicketReaderEditState {
  detailsMode: 'preview' | 'write'; detailsDraft: string; detailsBase: string; detailsGeneration: number;
  editingNoteId?: string; noteDraft: string; noteBase: string; noteGeneration: number;
  blockedReasonEditing: boolean; blockedReasonDraft: string; blockedReasonBase: string; blockedReasonGeneration: number;
}

export function ticketReaderEditState(ticket: FullTicket): TicketReaderEditState {
  return {detailsMode:'preview',detailsDraft:ticket.details,detailsBase:ticket.details,detailsGeneration:0,noteDraft:'',noteBase:'',noteGeneration:0,blockedReasonEditing:false,blockedReasonDraft:ticket.blocked_reason??'',blockedReasonBase:ticket.blocked_reason??'',blockedReasonGeneration:0};
}

export function updateTicketReaderFrame(stack: readonly TicketReaderFrame[], id: string, update: (frame: TicketReaderFrame) => TicketReaderFrame): TicketReaderFrame[] {
  return stack.map(frame => frame.id === id ? update(frame) : frame);
}

export function reconcileTicketReaderFrame(frame: TicketReaderFrame, ticket: FullTicket): TicketReaderFrame {
  const edit=frame.edit,note=edit.editingNoteId?ticket.notes.find(item=>item.id===edit.editingNoteId):undefined;
  return {...frame,ticket,edit:{...edit,detailsBase:ticket.details,detailsDraft:edit.detailsMode==='write'&&edit.detailsDraft!==edit.detailsBase?edit.detailsDraft:ticket.details,blockedReasonBase:ticket.blocked_reason??'',blockedReasonDraft:edit.blockedReasonEditing&&edit.blockedReasonDraft!==edit.blockedReasonBase?edit.blockedReasonDraft:ticket.blocked_reason??'',noteBase:note?.text??'',noteDraft:edit.editingNoteId&&edit.noteDraft!==edit.noteBase?edit.noteDraft:note?.text??''}};
}

export function pushTicketReaderFrame(stack: readonly TicketReaderFrame[], frame: TicketReaderFrame): TicketReaderFrame[] {
  return [...stack, frame];
}

export function popTicketReaderFrame(stack: readonly TicketReaderFrame[]): { stack: TicketReaderFrame[]; closed?: TicketReaderFrame } {
  if (stack.length === 0) return { stack: [] };
  return { stack: stack.slice(0, -1), closed: stack.at(-1) };
}

export function activeTicketReaderProject(stack: readonly TicketReaderFrame[], fallbackProjectId: string): string {
  return stack.at(-1)?.projectId ?? fallbackProjectId;
}
