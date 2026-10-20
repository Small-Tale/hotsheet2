import type { Note } from './api';

type RelevantNote = Pick<Note, 'id' | 'kind' | 'created_at'> &
  Partial<Pick<Note, 'text' | 'feedback_for' | 'ai_feedback' | 'feedback_request' | 'response_to'>>;
type FullFeedbackTicket = { feedback_needed?: boolean; details: string; notes: readonly RelevantNote[] };

export const DETAILS_FEEDBACK_ID = 'ticket-details';

/** Match the intentionally case-sensitive HS1 feedback marker used by the core.
 * Markdown blockquote lines are ignored so a reply that quotes the original request
 * back is not mistaken for a new feedback request (HS2-HG7FZ0); mirrors the core
 * `Note::text_requests_feedback`. */
export function textRequestsFeedback(text: string) {
  return text.split('\n').some((line) => !line.trimStart().startsWith('>') && line.includes('FEEDBACK NEEDED'));
}

export function isAiThumbsFeedback(note: Partial<Pick<Note, 'text' | 'feedback_for' | 'ai_feedback'>>): boolean {
  return Boolean(
    note.ai_feedback ||
    note.feedback_for ||
    /^AI feedback for .+: (Helpful — keep suggestions like this\.|Not helpful — stop suggestions like this\.)$/u.test(
      note.text?.split(/\r?\n/, 1)[0] ?? '',
    ),
  );
}

const after = (candidate: RelevantNote, note: RelevantNote) =>
  candidate.created_at > note.created_at || (candidate.created_at === note.created_at && candidate.id > note.id);

/** Keep new requests open independently while reading unmarked legacy notes sequentially. */
export function isFeedbackNeeded(notes: readonly RelevantNote[]) {
  if (
    notes.some(
      (request) =>
        request.kind === 'feedback_needed' &&
        request.feedback_request &&
        !notes.some((reply) => reply.response_to === request.id),
    )
  )
    return true;
  let latest: RelevantNote | undefined;
  for (const note of notes) {
    if (
      (note.kind !== 'regular' && note.kind !== 'feedback_needed') ||
      isAiThumbsFeedback(note) ||
      note.feedback_request ||
      note.response_to
    )
      continue;
    if (
      !latest ||
      note.created_at > latest.created_at ||
      (note.created_at === latest.created_at && note.id > latest.id)
    )
      latest = note;
  }
  return latest?.kind === 'feedback_needed' && !notes.some((reply) => reply.response_to === latest.id);
}

/** Preserve response UX when a newer client reads a full ticket from an older server
 * that predates the additive `feedback_needed` field on that wire shape. */
export function fullTicketFeedbackNeeded(ticket: FullFeedbackTicket): boolean {
  if (typeof ticket.feedback_needed === 'boolean') return ticket.feedback_needed;
  const hasExchange = ticket.notes.some(
    (note) => (note.kind === 'regular' || note.kind === 'feedback_needed') && !isAiThumbsFeedback(note),
  );
  return hasExchange ? isFeedbackNeeded(ticket.notes) : textRequestsFeedback(ticket.details);
}

/** Present each answered request as an ordinary note, preserving legacy response order. */
export function presentedNoteKind(note: RelevantNote, notes: readonly RelevantNote[]): RelevantNote['kind'] {
  if (note.kind === 'feedback_needed' && note.feedback_request) {
    return notes.some((reply) => reply.response_to === note.id) ? 'regular' : 'feedback_needed';
  }
  return note.kind === 'feedback_needed' &&
    (notes.some((candidate) => candidate.response_to === note.id) ||
      notes.some(
        (candidate) =>
          candidate.kind === 'regular' &&
          !candidate.response_to &&
          !isAiThumbsFeedback(candidate) &&
          after(candidate, note),
      ))
    ? 'regular'
    : note.kind;
}
