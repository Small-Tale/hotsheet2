import type { Note } from './api';

type RelevantNote = Pick<Note, 'id' | 'kind' | 'created_at'>;
type FullFeedbackTicket = { feedback_needed?: boolean; details: string; notes: readonly RelevantNote[] };

export const DETAILS_FEEDBACK_ID = 'ticket-details';

/** Match the intentionally case-sensitive HS1 feedback marker used by the core.
 * Markdown blockquote lines are ignored so a reply that quotes the original request
 * back is not mistaken for a new feedback request (HS2-HG7FZ0); mirrors the core
 * `Note::text_requests_feedback`. */
export function textRequestsFeedback(text: string) {
  return text.split('\n').some((line) => !line.trimStart().startsWith('>') && line.includes('FEEDBACK NEEDED'));
}

const after = (candidate: RelevantNote, note: RelevantNote) =>
  candidate.created_at > note.created_at || (candidate.created_at === note.created_at && candidate.id > note.id);

/** A regular note answers the preceding feedback request; other note kinds are neutral. */
export function isFeedbackNeeded(notes: readonly RelevantNote[]) {
  let latest: RelevantNote | undefined;
  for (const note of notes) {
    if (note.kind !== 'regular' && note.kind !== 'feedback_needed') continue;
    if (
      !latest ||
      note.created_at > latest.created_at ||
      (note.created_at === latest.created_at && note.id > latest.id)
    )
      latest = note;
  }
  return latest?.kind === 'feedback_needed';
}

/** Preserve response UX when a newer client reads a full ticket from an older server
 * that predates the additive `feedback_needed` field on that wire shape. */
export function fullTicketFeedbackNeeded(ticket: FullFeedbackTicket): boolean {
  if (typeof ticket.feedback_needed === 'boolean') return ticket.feedback_needed;
  const hasExchange = ticket.notes.some((note) => note.kind === 'regular' || note.kind === 'feedback_needed');
  return hasExchange ? isFeedbackNeeded(ticket.notes) : textRequestsFeedback(ticket.details);
}

/** A feedback ask that has already received a later regular response reads as an ordinary note. */
export function presentedNoteKind(note: RelevantNote, notes: readonly RelevantNote[]): RelevantNote['kind'] {
  return note.kind === 'feedback_needed' &&
    notes.some((candidate) => candidate.kind === 'regular' && after(candidate, note))
    ? 'regular'
    : note.kind;
}
