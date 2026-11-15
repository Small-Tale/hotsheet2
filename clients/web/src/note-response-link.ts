import type { Capabilities, Note } from './api';
import { isAiThumbsFeedback } from './feedback-needed';

type LinkNote = Pick<Note, 'id' | 'kind' | 'created_at' | 'text' | 'response_to'> &
  Partial<Pick<Note, 'feedback_for' | 'ai_feedback'>>;

/** Order notes as the core does: creation time, then id (HS2-J4G9GB). */
function chronological(a: LinkNote, b: LinkNote): number {
  const at = Date.parse(a.created_at) - Date.parse(b.created_at);
  if (at) return at;
  if (a.created_at !== b.created_at) return a.created_at < b.created_at ? -1 : 1;
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

/**
 * The feedback requests an existing note may answer (HS2-J4G9GB): a presented request on the same
 * ticket created before the note. Mirrors the core `validate_note_response_link`.
 */
export function responseLinkTargets<T extends LinkNote>(notes: readonly T[], noteId: string): T[] {
  const note = notes.find((item) => item.id === noteId);
  if (!note) return [];
  return notes.filter(
    (request) => request.id !== noteId && request.kind === 'feedback_needed' && chronological(request, note) < 0,
  );
}

/**
 * Why `noteId` cannot be linked as a response right now, or undefined when it can. `kind` is the
 * note's kind after a change made in the same edit. A linked note can always be unlinked or relinked.
 */
export function responseLinkUnavailable(
  notes: readonly LinkNote[],
  noteId: string,
  capabilities: Partial<Capabilities> | undefined,
  kind?: Note['kind'],
): string | undefined {
  if (!capabilities?.note_edit || !capabilities.note_response_link)
    return 'This source cannot link an existing note as a feedback response.';
  const note = notes.find((item) => item.id === noteId);
  if (!note || (kind ?? note.kind) !== 'regular' || isAiThumbsFeedback(note))
    return 'Only a regular note can answer a feedback request.';
  if (!note.response_to && responseLinkTargets(notes, noteId).length === 0)
    return 'No earlier feedback request on this ticket.';
  return undefined;
}

/** A short, plain label that identifies one request in a picker. */
export function responseLinkLabel(request: Pick<Note, 'text'>, maxLength = 72): string {
  const line =
    request.text
      .split(/\r?\n/u)
      .map((value) =>
        value
          .replace(/^[\s>#*_-]*/u, '')
          .replace(/^FEEDBACK NEEDED[*_:\s—–-]*/iu, '')
          .trim(),
      )
      .find(Boolean) ?? '';
  const label = line || 'Feedback request';
  return label.length > maxLength ? `${label.slice(0, maxLength - 1).trimEnd()}…` : label;
}
